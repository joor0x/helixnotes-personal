import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';

const source = await readFile(new URL('../src/lib/components/Editor.svelte', import.meta.url), 'utf8');

test('a checked task styles only its own checkbox, never its nested subtasks', () => {
  const rules = [...source.matchAll(/li\[data-checked="true"\][^{]*input\[type="checkbox"\][^{]*\{/g)].map(m => m[0]);
  assert.ok(rules.length >= 2, 'checked checkbox rules exist');
  // `li[data-checked="true"] label` also matches the labels of unchecked subtasks inside it.
  for (const rule of rules) assert.match(rule, /li\[data-checked="true"\] > label input/, rule);
});
