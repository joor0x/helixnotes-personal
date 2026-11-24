import assert from 'node:assert/strict';
import test from 'node:test';
import { Schema } from '@tiptap/pm/model';
import { buildBatches, buildDecorations, cacheKey, canonicalNoteLanguage, collectBlocks, matchRange, noteLanguage, splitMatches } from '../src/lib/editor/languagetool.ts';

const schema = new Schema({
  nodes: {
    doc: { content: 'block+' },
    paragraph: { group: 'block', content: 'inline*' },
    codeBlock: { group: 'block', content: 'text*', code: true, marks: '' },
    text: { group: 'inline' },
    hardBreak: { group: 'inline', inline: true },
    image: { group: 'inline', inline: true, atom: true },
  },
  marks: { bold: {}, code: { code: true } },
});

const p = (...content) => schema.node('paragraph', null, content);
const t = (text, marks = []) => schema.text(text, marks.map(m => schema.mark(m)));
const match = (offset, length, extra = {}) => ({ offset, length, message: 'm', shortMessage: '', replacements: ['x'], ruleId: 'R', issueType: 'misspelling', ...extra });

test('code blocks are skipped and inline code becomes markup', () => {
  const doc = schema.node('doc', null, [
    p(t('Run '), t('ls -la', ['code']), t(' now')),
    schema.node('codeBlock', null, [schema.text('const x = 1')]),
  ]);
  const blocks = collectBlocks(doc);
  assert.equal(blocks.length, 1);
  assert.deepEqual(blocks[0].items, [{ text: 'Run ' }, { markup: 'ls -la', interpretAs: '' }, { text: ' now' }]);
});

test('matches map to editor positions across marks but never into markup', () => {
  // doc: <p>Ella no <b>sab</b>ia `x`</p>; paragraph content starts at position 1.
  const doc = schema.node('doc', null, [p(t('Ella no '), t('sab', ['bold']), t('ia '), t('x', ['code']))]);
  const [block] = collectBlocks(doc);
  const range = matchRange(block, match(8, 5));
  assert.deepEqual(range, { from: 9, to: 14 });
  assert.equal(doc.textBetween(range.from, range.to), 'sabia');
  assert.equal(matchRange(block, match(12, 3)), null, 'touches inline code');
});

test('hard breaks and inline nodes keep later offsets aligned', () => {
  const doc = schema.node('doc', null, [p(t('Hola'), schema.node('hardBreak'), schema.node('image'), t('mundo malo'))]);
  const [block] = collectBlocks(doc);
  assert.equal(block.items[1].interpretAs, '\n');
  const range = matchRange(block, match(6, 5));
  assert.equal(doc.textBetween(range.from, range.to), 'mundo');
});

test('batches respect the size limit and matches return to their own block', () => {
  const doc = schema.node('doc', null, [p(t('Primer bloque')), p(t('Segundo bloque'))]);
  const blocks = collectBlocks(doc);
  const [batch] = buildBatches(blocks);
  assert.deepEqual(batch.starts, [0, 'Primer bloque'.length + 2]);
  const data = JSON.parse(batch.data);
  assert.equal(data.annotation.map(i => i.text ?? i.markup).join(''), 'Primer bloque\n\nSegundo bloque');
  const split = splitMatches([match(0, 6), match(15, 7), match(10, 8)], batch.starts, blocks);
  assert.deepEqual(split.map(b => b.map(m => m.offset)), [[0], [0]], 'cross-block match dropped');
  assert.equal(buildBatches(blocks, 15).length, 2);
});

test('very short or empty blocks are not sent', () => {
  const doc = schema.node('doc', null, [p(t('Ok')), p(), p(t('Texto suficiente'))]);
  assert.equal(collectBlocks(doc).length, 1);
});

test('decorations come from cached block results and skip ignored text', () => {
  const doc = schema.node('doc', null, [p(t('Ella no sabia'))]);
  const [block] = collectBlocks(doc);
  const cache = new Map([[cacheKey(null, block), [match(8, 5)]]]);
  const decorations = buildDecorations(doc, cache, new Set()).find();
  assert.equal(decorations.length, 1);
  assert.equal(decorations[0].from, 9);
  assert.equal(buildDecorations(doc, cache, new Set(['R\u0000sabia'])).find().length, 0);
});

test('note language comes from top-level lang or language frontmatter keys', () => {
  assert.equal(noteLanguage('---\ntitle: Nota\nlang: ca\n---\n'), 'ca');
  assert.equal(noteLanguage('---\nlanguage: "en-GB" # UK spelling\n---\n'), 'en-GB');
  assert.equal(noteLanguage("---\nlang: 'es_ES'\n---\n"), 'es_ES');
  assert.equal(noteLanguage('---\nlang: auto\n---\n'), 'auto');
  assert.equal(noteLanguage('---\nmeta:\n  lang: ca\n---\n'), null, 'nested keys are ignored');
  assert.equal(noteLanguage('---\nlang: català\n---\n'), null);
  assert.equal(noteLanguage(''), null);
  assert.equal(noteLanguage(null), null);
});

test('results are cached per language', () => {
  const doc = schema.node('doc', null, [p(t('Ella no sabia'))]);
  const [block] = collectBlocks(doc);
  assert.notEqual(cacheKey('ca', block), cacheKey(null, block));
  const cache = new Map([[cacheKey('es', block), [match(8, 5)]]]);
  assert.equal(buildDecorations(doc, cache, new Set(), 'es').find().length, 1);
  assert.equal(buildDecorations(doc, cache, new Set(), 'ca').find().length, 0);
});

test('selector values match how the backend stores note languages', () => {
  assert.equal(canonicalNoteLanguage('ca'), 'ca-ES');
  assert.equal(canonicalNoteLanguage('EN'), 'en-US');
  assert.equal(canonicalNoteLanguage('en_gb'), 'en-GB');
  assert.equal(canonicalNoteLanguage('es'), 'es');
  assert.equal(canonicalNoteLanguage('ca-es-VALENCIA'), 'ca-ES-valencia');
  assert.equal(canonicalNoteLanguage(null), null);
});
