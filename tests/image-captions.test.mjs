import assert from 'node:assert/strict';
import test from 'node:test';
import { Schema } from '@tiptap/pm/model';
import { EditorState } from '@tiptap/pm/state';
import { captionDecorations, touchesImages } from '../src/lib/editor/extensions/imageCaptions.ts';

const schema = new Schema({
  nodes: {
    doc: { content: 'block+' },
    paragraph: { group: 'block', content: 'inline*' },
    imageGrid: { group: 'block', content: 'image+' },
    text: { group: 'inline' },
    image: { group: 'inline', inline: true, atom: true, attrs: { src: { default: '' }, title: { default: null } } },
  },
});
const image = (title) => schema.node('image', { src: 'a.png', title });
const p = (...content) => schema.node('paragraph', null, content);

test('images with a title get a caption, except inside grids', () => {
  const doc = schema.node('doc', null, [
    p(image('Pie de foto')),
    p(image(null)),
    p(image('  ')),
    schema.node('imageGrid', null, [image('En la cuadrícula')]),
  ]);
  const found = captionDecorations(doc).find();
  assert.equal(found.length, 1);
  assert.equal(found[0].from, 2, 'placed right after the first image');
});

test('only edits that touch an image rebuild the captions', () => {
  const doc = schema.node('doc', null, [p(schema.text('Texto')), p(image('Pie'))]);
  const state = EditorState.create({ doc });
  assert.equal(touchesImages(state.tr.insertText('!', 3)), false);
  assert.equal(touchesImages(state.tr.setNodeAttribute(8, 'title', 'Otro')), true);
  assert.equal(touchesImages(state.tr.delete(8, 9)), true);
});
