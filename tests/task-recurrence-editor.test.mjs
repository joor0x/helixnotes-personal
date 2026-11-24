import assert from 'node:assert/strict';
import test from 'node:test';
import { Schema } from '@tiptap/pm/model';
import { EditorState } from '@tiptap/pm/state';
import { history, undo, redo } from '@tiptap/pm/history';
import { taskRecurrencePlugin } from '../src/lib/editor/recurrence.ts';

const schema = new Schema({
  nodes: {
    doc: { content: 'block+' },
    paragraph: { group: 'block', content: 'inline*' },
    taskList: { group: 'block', content: 'taskItem+' },
    taskItem: { content: 'paragraph block*', attrs: { checked: { default: false } } },
    text: { group: 'inline' },
  },
  marks: { bold: {} },
});
const p = (...content) => schema.node('paragraph', null, content.map(c => typeof c === 'string' ? schema.text(c) : c));
const item = (checked, ...children) => schema.node('taskItem', { checked }, children);
const list = (...items) => schema.node('taskList', null, items);
const now = () => new Date(2026, 10, 30, 15, 0);

function stateWith(doc) {
  return EditorState.create({ doc, plugins: [history(), taskRecurrencePlugin(now)] });
}

function toggle(state, index = 0) {
  let target = -1;
  let seen = 0;
  state.doc.descendants((node, pos) => {
    if (node.type.name === 'taskItem' && seen++ === index) target = pos;
  });
  const node = state.doc.nodeAt(target);
  return state.apply(state.tr.setNodeMarkup(target, undefined, { ...node.attrs, checked: !node.attrs.checked }));
}

const tasks = (state) => {
  const result = [];
  state.doc.descendants((node) => {
    if (node.type.name === 'taskItem') result.push(`${node.attrs.checked ? 'x' : ' '} ${node.firstChild.textContent}`);
  });
  return result;
};

test('ticking a recurring task opens its next occurrence above it, without its subtasks', () => {
  const state = stateWith(schema.node('doc', null, [list(
    item(false, p('Review ', schema.text('budget', [schema.marks.bold.create()]), ' due:2026-11-30 remind:2026-11-30T09:00 repeat:weekly'),
      list(item(false, p('Export CSV')))),
    item(false, p('One-off')),
  )]));
  const next = toggle(state);
  assert.deepEqual(tasks(next), [
    '  Review budget due:2026-12-07 remind:2026-12-07T09:00 repeat:weekly',
    'x Review budget due:2026-11-30 remind:2026-11-30T09:00 repeat:weekly',
    '  Export CSV',
    '  One-off',
  ]);
  // Marks survive in the copy.
  const copy = next.doc.firstChild.firstChild.firstChild;
  assert.ok(copy.child(1).marks.some(mark => mark.type.name === 'bold'));

  // Undo removes both the completion and the new occurrence; redo does not duplicate it.
  const undone = (() => { let s = next; undo(s, tr => { s = s.apply(tr); }); return s; })();
  assert.deepEqual(tasks(undone), tasks(state));
  const redone = (() => { let s = undone; redo(s, tr => { s = s.apply(tr); }); return s; })();
  assert.deepEqual(tasks(redone), tasks(next));
});

test('unchecking or ticking a one-off task never adds tasks', () => {
  const state = stateWith(schema.node('doc', null, [list(item(true, p('Done repeat:daily due:2026-11-29')), item(false, p('One-off')))]));
  assert.equal(tasks(toggle(state, 0)).length, 2);
  assert.equal(tasks(toggle(state, 1)).length, 2);
});

test('an undated recurring task gets its first due date', () => {
  const state = stateWith(schema.node('doc', null, [list(item(false, p('Water plants repeat:3d')))]));
  assert.deepEqual(tasks(toggle(state)), ['  Water plants repeat:3d due:2026-12-03', 'x Water plants repeat:3d']);
});
