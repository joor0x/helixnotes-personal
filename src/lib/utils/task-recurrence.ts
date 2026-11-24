/**
 * Recurring tasks: `repeat:daily|weekly|monthly|yearly|weekdays` or `repeat:<N><d|w|m|y>`.
 * Completing one opens its next occurrence. Mirrors `src-tauri/src/reminders.rs`, which
 * does the same for the tasks view; the editor uses this copy for checkbox clicks.
 */

import type { MessageKey } from '../i18n/core.ts';

export type RepeatUnit = 'd' | 'w' | 'm' | 'y' | 'weekdays';

export interface RepeatRule {
  unit: RepeatUnit;
  count: number;
}

/** Choices offered by the pickers, in their canonical spelling. */
export const REPEAT_PRESETS = ['daily', 'weekdays', 'weekly', '2w', 'monthly', 'yearly'] as const;

const REPEAT_TOKEN = /(^|\s)repeat:(daily|weekly|monthly|yearly|weekdays|[1-9]\d{0,2}[dwmy])\b/i;
const DUE_TOKEN = /\bdue:(\d{4}-\d{2}-\d{2})(?:T(\d{2}:\d{2}))?\b/;
const REMIND_TOKEN = /\bremind:(\d{4}-\d{2}-\d{2})T(\d{2}:\d{2})\b/;
/** Upper bound on skipped occurrences, as in the Rust side. */
const MAX_SKIPPED_PERIODS = 20_000;

const NAMED: Record<string, RepeatRule> = {
  daily: { unit: 'd', count: 1 },
  weekly: { unit: 'w', count: 1 },
  monthly: { unit: 'm', count: 1 },
  yearly: { unit: 'y', count: 1 },
  weekdays: { unit: 'weekdays', count: 1 },
};

export function parseRepeat(value: string | null | undefined): RepeatRule | null {
  const text = (value ?? '').trim().toLowerCase();
  if (NAMED[text]) return { ...NAMED[text] };
  const match = /^([1-9]\d{0,2})([dwmy])$/.exec(text);
  return match ? { unit: match[2] as RepeatUnit, count: Number(match[1]) } : null;
}

export function canonicalRepeat(rule: RepeatRule): string {
  if (rule.unit === 'weekdays') return 'weekdays';
  if (rule.count === 1) return { d: 'daily', w: 'weekly', m: 'monthly', y: 'yearly' }[rule.unit];
  return `${rule.count}${rule.unit}`;
}

/** The rule of the `repeat:` token in a task's text. */
export function repeatOf(text: string): RepeatRule | null {
  const match = REPEAT_TOKEN.exec(text);
  return match ? parseRepeat(match[2]) : null;
}

// Calendar dates as UTC midnights, so arithmetic never meets a DST shift.
function toDay(ymd: string): Date | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(ymd);
  if (!match) return null;
  const date = new Date(Date.UTC(+match[1], +match[2] - 1, +match[3]));
  return date.getUTCDate() === +match[3] ? date : null;
}

function fromDay(date: Date): string {
  return date.toISOString().slice(0, 10);
}

function addMonths(date: Date, months: number): Date {
  const year = date.getUTCFullYear();
  const month = date.getUTCMonth() + months;
  const lastDay = new Date(Date.UTC(year, month + 1, 0)).getUTCDate();
  return new Date(Date.UTC(year, month, Math.min(date.getUTCDate(), lastDay)));
}

/** `ymd` moved forward by `periods` occurrences; month ends are clamped, never drift. */
export function advanceDate(ymd: string, rule: RepeatRule, periods: number): string | null {
  const date = toDay(ymd);
  if (!date) return null;
  const day = 86_400_000;
  switch (rule.unit) {
    case 'd': return fromDay(new Date(date.getTime() + rule.count * periods * day));
    case 'w': return fromDay(new Date(date.getTime() + 7 * rule.count * periods * day));
    case 'm': return fromDay(addMonths(date, rule.count * periods));
    case 'y': return fromDay(addMonths(date, 12 * rule.count * periods));
    case 'weekdays': {
      let time = date.getTime();
      for (let i = 0; i < periods; i++) {
        do time += day; while ([0, 6].includes(new Date(time).getUTCDay()));
      }
      return fromDay(new Date(time));
    }
  }
}

function localStamp(now: Date): { today: string; time: string } {
  const pad = (n: number) => String(n).padStart(2, '0');
  return {
    today: `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`,
    time: `${pad(now.getHours())}:${pad(now.getMinutes())}`,
  };
}

export interface Occurrence {
  /** `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM`. */
  due: string | null;
  remind: string | null;
}

/**
 * The next occurrence after completing a task at `now` (local time). Due date and reminder
 * move by the same number of periods, the fewest that put both in the future, so the new
 * occurrence is not born overdue and its reminder does not fire at once.
 */
export function nextOccurrence(
  due: string | null,
  dueTime: string | null,
  remind: string | null,
  rule: RepeatRule,
  now: Date,
): Occurrence | null {
  const { today, time } = localStamp(now);
  const nowStamp = `${today}T${time}`;
  const dueDate = due && toDay(due) ? due : null;
  const [remindDate, remindTime] = remind?.split('T') ?? [];
  const reminder = remindDate && toDay(remindDate) && /^\d{2}:\d{2}$/.test(remindTime ?? '') ? remind : null;
  if (!dueDate && !reminder) {
    const next = advanceDate(today, rule, 1);
    return next ? { due: next, remind: null } : null;
  }
  let result: Occurrence | null = null;
  for (let periods = 1; periods <= MAX_SKIPPED_PERIODS; periods++) {
    const nextDue = dueDate ? advanceDate(dueDate, rule, periods) : null;
    const nextRemind = reminder ? `${advanceDate(remindDate, rule, periods)}T${remindTime}` : null;
    result = { due: nextDue && dueTime ? `${nextDue}T${dueTime}` : nextDue, remind: nextRemind };
    const dueOk = !nextDue || (dueTime ? `${nextDue}T${dueTime}` > nowStamp : nextDue >= today);
    const remindOk = !nextRemind || nextRemind > nowStamp;
    if (dueOk && remindOk) break;
  }
  return result;
}

export interface TokenEdit {
  /** Offsets in the text the edit was computed from. */
  from: number;
  to: number;
  insert: string;
}

/**
 * Edits that turn a recurring task's text into its next occurrence's text: each date token
 * keeps its place with a new value, and an undated task gains a `due:` at the end.
 * `null` when the text has no `repeat:` rule.
 */
export function nextOccurrenceEdits(text: string, now: Date): TokenEdit[] | null {
  const rule = repeatOf(text);
  if (!rule) return null;
  const due = DUE_TOKEN.exec(text);
  const remind = REMIND_TOKEN.exec(text);
  const next = nextOccurrence(
    due?.[1] ?? null,
    due?.[2] ?? null,
    remind ? `${remind[1]}T${remind[2]}` : null,
    rule,
    now,
  );
  if (!next) return null;
  const edits: TokenEdit[] = [];
  if (due && next.due) edits.push({ from: due.index, to: due.index + due[0].length, insert: `due:${next.due}` });
  if (remind && next.remind) edits.push({ from: remind.index, to: remind.index + remind[0].length, insert: `remind:${next.remind}` });
  if (!due && next.due) {
    const end = text.trimEnd().length;
    edits.push({ from: end, to: end, insert: ` due:${next.due}` });
  }
  return edits.sort((a, b) => b.from - a.from);
}

/** The text of the next occurrence (see {@link nextOccurrenceEdits}). */
export function nextOccurrenceText(text: string, now: Date): string | null {
  const edits = nextOccurrenceEdits(text, now);
  if (!edits) return null;
  let result = text;
  for (const edit of edits) result = result.slice(0, edit.from) + edit.insert + result.slice(edit.to);
  return result;
}

export interface RepeatLabels {
  daily: string;
  weekdays: string;
  weekly: string;
  monthly: string;
  yearly: string;
  /** Called with the count for `every N days/weeks/months/years`. */
  every: (count: number, unit: 'd' | 'w' | 'm' | 'y') => string;
}

type Translate = (key: MessageKey, values?: Record<string, unknown>) => string;

/** {@link RepeatLabels} from the UI catalog (`tasks.repeat.*`). */
export function repeatLabelsFrom(t: Translate): RepeatLabels {
  const every = { d: 'everyDays', w: 'everyWeeks', m: 'everyMonths', y: 'everyYears' } as const;
  return {
    daily: t('tasks.repeat.daily'),
    weekdays: t('tasks.repeat.weekdays'),
    weekly: t('tasks.repeat.weekly'),
    monthly: t('tasks.repeat.monthly'),
    yearly: t('tasks.repeat.yearly'),
    every: (count, unit) => t(`tasks.repeat.${every[unit]}` as MessageKey, { count }),
  };
}

/** A human label for a canonical repeat value (`weekly`, `3d`, ...). */
export function repeatLabel(value: string, labels: RepeatLabels): string {
  const rule = parseRepeat(value);
  if (!rule) return value;
  if (rule.unit === 'weekdays') return labels.weekdays;
  if (rule.count === 1) return labels[canonicalRepeat(rule) as 'daily' | 'weekly' | 'monthly' | 'yearly'];
  return labels.every(rule.count, rule.unit);
}
