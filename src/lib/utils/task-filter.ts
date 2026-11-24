export type TaskDueFilter = 'all' | 'today' | 'overdue';

/** Overdue means late and still open: a completed task is never overdue. */
export function matchesTaskDueFilter(due: string | null, filter: TaskDueFilter, today: string, completed = false): boolean {
  if (filter === 'all') return true;
  if (!due) return false;
  return filter === 'today' ? due === today : !completed && due < today;
}

/** Local `HH:MM` for comparisons with task due times. */
export function formatLocalTime(date: Date): string {
  return `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`;
}

/** Sort key: date, then time; a date without time sorts after timed tasks of that day. */
export function taskDueKey(due: string | null, dueTime: string | null): string {
  return due ? `${due}T${dueTime ?? '99:99'}` : '9999-99-99';
}

/** Late and still open, down to the minute when the task has a due time. */
export function isTaskOverdue(due: string | null, dueTime: string | null, completed: boolean, today: string, nowTime: string): boolean {
  if (!due || completed) return false;
  return due < today || (due === today && !!dueTime && dueTime < nowTime);
}

/** Tags are compared case-insensitively; an empty filter matches every task. */
export function matchesTaskTag(tags: readonly string[] | undefined, tag: string): boolean {
  if (!tag) return true;
  const wanted = tag.toLowerCase();
  return (tags ?? []).some(t => t.toLowerCase() === wanted);
}

/** Distinct tags across tasks (first spelling wins), sorted for display. */
export function collectTaskTags(tasks: readonly { tags?: readonly string[] }[]): string[] {
  const seen = new Map<string, string>();
  for (const task of tasks) for (const tag of task.tags ?? []) {
    if (!seen.has(tag.toLowerCase())) seen.set(tag.toLowerCase(), tag);
  }
  return [...seen.values()].sort((a, b) => a.localeCompare(b));
}

/** Value for `due:` from a date input and an optional time input. */
export function composeDue(date: string, time: string): string | null {
  if (!date) return null;
  return time ? `${date}T${time}` : date;
}
