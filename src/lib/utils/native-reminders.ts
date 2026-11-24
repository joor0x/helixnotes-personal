import type { TaskItem } from '../types';
import { repeatLabel, type RepeatLabels } from './task-recurrence.ts';

/** A reminder as the Android scheduler stores it (see `Reminders.kt`). */
export interface NativeReminder {
  id: string;
  /** Local wall-clock time, `YYYY-MM-DDTHH:MM`. */
  at: string;
  title: string;
  body: string;
  path: string;
}

export interface NativeReminderLabels {
  channel: string;
  snooze10: string;
  snooze60: string;
  snoozeTomorrow: string;
}

/**
 * Completing a recurring task replaces it with its next occurrence, whose new `remind:` time
 * makes a new id: the sync cancels the old alarm and notification and schedules the new one.
 */

/**
 * Identifies a reminder across syncs without line numbers, which shift as the note is
 * edited. Changing the task text or its time makes a new reminder, so it can fire again.
 */
export function reminderId(task: Pick<TaskItem, 'note_path' | 'text' | 'remind'>): string {
  return `${task.note_path}\n${task.remind}\n${task.text.trim()}`;
}

/** Open tasks with a `remind:` time; identical task lines in one note count once. */
export function nativeReminders(tasks: readonly TaskItem[], repeatLabels?: RepeatLabels): NativeReminder[] {
  const reminders = new Map<string, NativeReminder>();
  for (const task of tasks) {
    if (task.completed || !task.remind) continue;
    const id = reminderId(task);
    if (reminders.has(id)) continue;
    const due = task.due ? ` · ${task.due}${task.due_time ? ` ${task.due_time}` : ''}` : '';
    const repeat = task.repeat ? ` · ↻ ${repeatLabels ? repeatLabel(task.repeat, repeatLabels) : task.repeat}` : '';
    reminders.set(id, {
      id,
      at: task.remind,
      title: task.text.trim() || task.note_title,
      body: `${task.note_title}${due}${repeat}`,
      path: task.note_path,
    });
  }
  return [...reminders.values()];
}

interface ReminderBridge {
  syncReminders?: (json: string) => void;
  takeReminderNote?: () => string;
}

function bridge(): ReminderBridge | undefined {
  return typeof window === 'undefined' ? undefined : (window as unknown as { Android?: ReminderBridge }).Android;
}

/** Hands the reminders to Android, replacing the ones it knew. False without the bridge. */
export function syncNativeReminders(
  tasks: readonly TaskItem[],
  labels: NativeReminderLabels,
  repeatLabels?: RepeatLabels,
): boolean {
  const android = bridge();
  if (typeof android?.syncReminders !== 'function') return false;
  android.syncReminders(JSON.stringify({ reminders: nativeReminders(tasks, repeatLabels), labels }));
  return true;
}

/** The note of the reminder notification the user tapped, once. */
export function takeReminderNote(): string | null {
  const android = bridge();
  return (typeof android?.takeReminderNote === 'function' && android.takeReminderNote()) || null;
}
