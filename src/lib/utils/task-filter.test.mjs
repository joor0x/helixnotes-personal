import assert from 'node:assert/strict';
import test from 'node:test';
import { matchesTaskDueFilter } from './task-filter.ts';
import { formatLocalDate } from './time.ts';

const today = '2027-01-01';
const dates = [null, '2026-12-31', today, '2027-01-02'];

test('Today includes only tasks due on the current local day', () => {
  assert.deepEqual(dates.filter(due => matchesTaskDueFilter(due, 'today', today)), [today]);
});

test('Overdue excludes today, future dates, and undated tasks across a year boundary', () => {
  assert.deepEqual(dates.filter(due => matchesTaskDueFilter(due, 'overdue', today)), ['2026-12-31']);
});

test('Overdue never includes a completed task, while Today and all still do', () => {
  assert.equal(matchesTaskDueFilter('2026-12-31', 'overdue', today, false), true);
  assert.equal(matchesTaskDueFilter('2026-12-31', 'overdue', today, true), false);
  assert.equal(matchesTaskDueFilter(today, 'today', today, true), true);
  assert.equal(matchesTaskDueFilter('2026-12-31', 'all', today, true), true);
});

test('clearing the date filter restores dated and undated tasks', () => {
  assert.equal(matchesTaskDueFilter(null, 'all', today), true);
  assert.equal(matchesTaskDueFilter('2027-01-02', 'all', today), true);
});

test('task date keys use the local calendar rather than a locale-specific display string', () => {
  assert.equal(formatLocalDate(new Date(2026, 11, 31, 23, 59)), '2026-12-31');
  assert.equal(formatLocalDate(new Date(2027, 0, 1, 0, 0)), '2027-01-01');
});

test('due times sort within the day and make a task overdue after that minute', async () => {
  const { taskDueKey, isTaskOverdue } = await import('./task-filter.ts');
  const keys = [taskDueKey('2027-01-01', null), taskDueKey('2027-01-01', '09:00'), taskDueKey(null, null), taskDueKey('2026-12-31', '23:00')];
  assert.deepEqual([...keys].sort(), ['2026-12-31T23:00', '2027-01-01T09:00', '2027-01-01T99:99', '9999-99-99']);
  assert.equal(isTaskOverdue(today, '09:00', false, today, '09:01'), true);
  assert.equal(isTaskOverdue(today, '09:00', false, today, '08:59'), false);
  assert.equal(isTaskOverdue(today, null, false, today, '23:59'), false);
  assert.equal(isTaskOverdue('2026-12-31', null, false, today, '00:00'), true);
  assert.equal(isTaskOverdue('2026-12-31', null, true, today, '00:00'), false);
});

test('tag filter is case-insensitive and lists each tag once', async () => {
  const { matchesTaskTag, collectTaskTags } = await import('./task-filter.ts');
  assert.equal(matchesTaskTag(['Work', 'home'], 'work'), true);
  assert.equal(matchesTaskTag(['home'], 'work'), false);
  assert.equal(matchesTaskTag(undefined, ''), true);
  assert.deepEqual(collectTaskTags([{ tags: ['work', 'Home'] }, { tags: ['home', 'client/acme'] }, {}]), ['client/acme', 'Home', 'work']);
});

test('due values combine date and optional time', async () => {
  const { composeDue } = await import('./task-filter.ts');
  assert.equal(composeDue('2027-01-01', ''), '2027-01-01');
  assert.equal(composeDue('2027-01-01', '14:30'), '2027-01-01T14:30');
  assert.equal(composeDue('', '14:30'), null);
});
