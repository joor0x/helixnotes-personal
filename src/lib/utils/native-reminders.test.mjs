import assert from 'node:assert/strict';
import test from 'node:test';
import { nativeReminders, reminderId } from './native-reminders.ts';

const task = (overrides = {}) => ({
  note_path: '/v/trabajo.md',
  note_title: 'Trabajo',
  line: 3,
  raw_line: '- [ ] Llamar',
  text: 'Llamar al banco',
  completed: false,
  due: null,
  due_time: null,
  remind: '2026-12-01T09:30',
  tags: [],
  priority: null,
  ...overrides,
});

test('only open tasks with a reminder are scheduled', () => {
  const reminders = nativeReminders([
    task(),
    task({ text: 'Hecha', completed: true }),
    task({ text: 'Sin aviso', remind: null }),
  ]);
  assert.deepEqual(reminders.map(r => r.title), ['Llamar al banco']);
  assert.equal(reminders[0].at, '2026-12-01T09:30');
  assert.equal(reminders[0].path, '/v/trabajo.md');
});

test('the body names the note and the due date', () => {
  assert.equal(nativeReminders([task()])[0].body, 'Trabajo');
  assert.equal(nativeReminders([task({ due: '2026-12-01', due_time: '10:00' })])[0].body, 'Trabajo · 2026-12-01 10:00');
});

test('the id survives a line move but not a new time or text', () => {
  const id = reminderId(task());
  assert.equal(reminderId(task({ line: 40 })), id);
  assert.notEqual(reminderId(task({ remind: '2026-12-01T09:40' })), id);
  assert.notEqual(reminderId(task({ text: 'Llamar al seguro' })), id);
  assert.notEqual(reminderId(task({ note_path: '/v/otra.md' })), id);
});

test('repeated task lines in a note produce one reminder', () => {
  assert.equal(nativeReminders([task(), task({ line: 9 })]).length, 1);
});
