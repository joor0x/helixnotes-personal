import type { Node as ProseMirrorNode } from '@tiptap/pm/model';
import { Plugin, PluginKey, type EditorState, type Transaction } from '@tiptap/pm/state';
import { AttrStep, ReplaceAroundStep } from '@tiptap/pm/transform';
import { nextOccurrenceEdits } from '../utils/task-recurrence.ts';

export const taskRecurrencePluginKey = new PluginKey('taskRecurrence');

/** Positions (in `state`) of task items a checkbox click just completed, last first. */
export function completedTaskItems(transactions: readonly Transaction[], state: EditorState): number[] {
  const found = new Set<number>();
  transactions.forEach((tr, index) => {
    // Undo and redo replay a completion together with the occurrence it created.
    if (tr.getMeta('history$') || tr.getMeta(taskRecurrencePluginKey)) return;
    tr.steps.forEach((step, stepIndex) => {
      // A checkbox toggle is `setNodeMarkup` (or `setNodeAttribute`) on the task item;
      // pasting or retyping a checked item is a plain replace and never repeats.
      let from: number;
      if (step instanceof ReplaceAroundStep) from = step.from;
      else if (step instanceof AttrStep) from = step.pos;
      else return;
      const before = tr.docs[stepIndex].nodeAt(from);
      if (before?.type.name !== 'taskItem' || before.attrs.checked) return;
      let pos = tr.mapping.slice(stepIndex).map(from);
      for (const later of transactions.slice(index + 1)) pos = later.mapping.map(pos);
      const after = state.doc.nodeAt(pos);
      if (after?.type.name === 'taskItem' && after.attrs.checked) found.add(pos);
    });
  });
  return [...found].sort((a, b) => b - a);
}

/** Position inside `block`'s content of character `offset` of its text. */
function textOffsetToPos(block: ProseMirrorNode, offset: number): number {
  let text = 0;
  let result = block.content.size;
  block.forEach((child, pos) => {
    if (result !== block.content.size) return;
    const length = child.isText ? child.text!.length : 0;
    if (child.isText && offset <= text + length) result = pos + offset - text;
    text += length;
  });
  return result;
}

/**
 * Open the next occurrence of the completed recurring task item at `pos` above it: a copy
 * of its first paragraph (without subtasks) with the date tokens moved forward.
 */
export function insertNextOccurrence(tr: Transaction, pos: number, now: Date): boolean {
  const item = tr.doc.nodeAt(pos);
  const paragraph = item?.firstChild;
  if (item?.type.name !== 'taskItem' || !paragraph?.isTextblock) return false;
  const edits = nextOccurrenceEdits(paragraph.textContent, now);
  if (!edits) return false;
  tr.insert(pos, item.type.create({ ...item.attrs, checked: false }, paragraph));
  const start = pos + 2; // inside the new task item and its paragraph
  for (const edit of edits) {
    // Edits come last first, so earlier offsets stay valid; text keeps its marks.
    const from = start + textOffsetToPos(paragraph, edit.from);
    const to = start + textOffsetToPos(paragraph, edit.to);
    tr.insertText(edit.insert, from, to);
  }
  return true;
}

/**
 * Completing a recurring task in the editor opens its next occurrence above it, like the
 * tasks view does; the saved note then reschedules its reminder (desktop and Android).
 */
export function taskRecurrencePlugin(now: () => Date = () => new Date()): Plugin {
  return new Plugin({
    key: taskRecurrencePluginKey,
    appendTransaction(transactions, _oldState, state) {
      if (!transactions.some(tr => tr.docChanged)) return null;
      const positions = completedTaskItems(transactions, state);
      if (!positions.length) return null;
      const tr = state.tr;
      let changed = false;
      for (const pos of positions) changed = insertNextOccurrence(tr, pos, now()) || changed;
      return changed ? tr.setMeta(taskRecurrencePluginKey, true) : null;
    },
  });
}
