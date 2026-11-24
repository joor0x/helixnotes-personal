import { Extension } from '@tiptap/core';
import type { Node as ProseMirrorNode } from '@tiptap/pm/model';
import { Plugin, PluginKey, type Transaction } from '@tiptap/pm/state';
import { Decoration, DecorationSet } from '@tiptap/pm/view';

/**
 * Shows an image's title (`![alt](src "Caption")`) as a caption below it. The caption is a
 * widget, not document content, so the Markdown stays a plain image and the editor keeps
 * treating the image as a single node. Images inside a grid keep their compact layout.
 */
export function captionDecorations(doc: ProseMirrorNode): DecorationSet {
	const decorations: Decoration[] = [];
	doc.descendants((node, pos, parent) => {
		if (node.type.name !== 'image') return;
		const caption = String(node.attrs.title ?? '').trim();
		if (!caption || parent?.type.name === 'imageGrid') return false;
		decorations.push(Decoration.widget(pos + node.nodeSize, () => {
			const element = document.createElement('span');
			element.className = 'image-caption';
			element.contentEditable = 'false';
			element.textContent = caption;
			return element;
		}, { side: -1, key: `image-caption:${caption}`, ignoreSelection: true }));
	});
	return DecorationSet.create(doc, decorations);
}

/** Whether a transaction may add, remove or retitle an image; typing elsewhere only maps the set. */
export function touchesImages(tr: Transaction): boolean {
	return tr.steps.some((step, index) => {
		if ('attr' in step) return true;
		const before = tr.docs[index];
		const after = tr.docs[index + 1] ?? tr.doc;
		let found = false;
		step.getMap().forEach((oldStart, oldEnd, newStart, newEnd) => {
			const check = (doc: ProseMirrorNode, from: number, to: number) => {
				if (found) return;
				doc.nodesBetween(Math.max(0, from - 1), Math.min(doc.content.size, to + 1), (node) => {
					if (node.type.name === 'image') found = true;
					return !found;
				});
			};
			check(before, oldStart, oldEnd);
			check(after, newStart, newEnd);
		});
		return found;
	});
}

const key = new PluginKey<DecorationSet>('imageCaptions');

export const ImageCaptions = Extension.create({
	name: 'imageCaptions',
	addProseMirrorPlugins() {
		return [new Plugin<DecorationSet>({
			key,
			state: {
				init: (_, { doc }) => captionDecorations(doc),
				apply: (tr, set) => {
					if (!tr.docChanged) return set;
					return touchesImages(tr) ? captionDecorations(tr.doc) : set.map(tr.mapping, tr.doc);
				},
			},
			props: { decorations: (state) => key.getState(state) },
		})];
	},
});
