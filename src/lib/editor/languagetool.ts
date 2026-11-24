import { Extension } from '@tiptap/core';
import type { Node as ProseMirrorNode } from '@tiptap/pm/model';
import { Plugin, PluginKey, type EditorState } from '@tiptap/pm/state';
import { Decoration, DecorationSet, type EditorView } from '@tiptap/pm/view';

/** One LanguageTool match, as returned by the `languagetool_check` command. */
export type LtMatch = {
	offset: number;
	length: number;
	message: string;
	shortMessage: string;
	replacements: string[];
	ruleId: string;
	issueType: string;
};

type AnnotationItem = { text: string } | { markup: string; interpretAs: string };
/** A run of the LanguageTool text. `pmStart` is null for markup (non-prose). */
type Segment = { ltStart: number; length: number; pmStart: number | null };

/** One prose textblock prepared for LanguageTool. Offsets are UTF-16, like ProseMirror. */
export type LtBlock = { key: string; items: AnnotationItem[]; segments: Segment[]; length: number };

const MIN_BLOCK_CHARS = 3;
/** The public API accepts 20 KB per request; stay below it. */
const MAX_BATCH_CHARS = 18_000;
const BLOCK_SEPARATOR = '\n\n';
const IGNORED_STORAGE_KEY = 'helixnotes.languagetool.ignored';

/**
 * Prose textblocks of a document. Code blocks are skipped; inline code, hard breaks and
 * inline nodes (images, wiki links, ...) become markup so LanguageTool never checks them,
 * yet its offsets still line up with editor positions.
 */
export function collectBlocks(doc: ProseMirrorNode): LtBlock[] {
	const blocks: LtBlock[] = [];
	doc.descendants((node, pos) => {
		if (!node.isTextblock) return true;
		if (node.type.spec.code) return false;
		const items: AnnotationItem[] = [];
		const segments: Segment[] = [];
		let lt = 0;
		let prose = 0;
		node.forEach((child, offset) => {
			const pmStart = pos + 1 + offset;
			if (child.isText && child.text && !child.marks.some(mark => mark.type.spec.code)) {
				items.push({ text: child.text });
				segments.push({ ltStart: lt, length: child.text.length, pmStart });
				lt += child.text.length;
				prose += child.text.trim().length;
			} else {
				const markup = child.isText && child.text ? child.text : '￼';
				items.push({ markup, interpretAs: child.type.name === 'hardBreak' ? '\n' : '' });
				segments.push({ ltStart: lt, length: markup.length, pmStart: null });
				lt += markup.length;
			}
		});
		if (prose >= MIN_BLOCK_CHARS) blocks.push({ key: JSON.stringify(items), items, segments, length: lt });
		return false;
	});
	return blocks;
}

/** Split blocks into request-sized batches, each with every block's offset in the batch. */
export function buildBatches(blocks: LtBlock[], maxChars = MAX_BATCH_CHARS) {
	const batches: { data: string; starts: number[]; blocks: LtBlock[] }[] = [];
	let items: AnnotationItem[] = [], starts: number[] = [], current: LtBlock[] = [], size = 0;
	const flush = () => {
		if (current.length) batches.push({ data: JSON.stringify({ annotation: items }), starts, blocks: current });
		items = []; starts = []; current = []; size = 0;
	};
	for (const block of blocks) {
		if (current.length && size + block.length > maxChars) flush();
		if (current.length) { items.push({ text: BLOCK_SEPARATOR }); size += BLOCK_SEPARATOR.length; }
		starts.push(size);
		current.push(block);
		items.push(...block.items);
		size += block.length;
	}
	flush();
	return batches;
}

/** Assign batch matches to their blocks, relative to each block; drops cross-block matches. */
export function splitMatches(matches: LtMatch[], starts: number[], blocks: LtBlock[]): LtMatch[][] {
	const result = blocks.map(() => [] as LtMatch[]);
	for (const match of matches) {
		const index = starts.findLastIndex(start => start <= match.offset);
		if (index < 0) continue;
		const offset = match.offset - starts[index];
		if (offset + match.length > blocks[index].length) continue;
		result[index].push({ ...match, offset });
	}
	return result;
}

/** Editor range of a block-relative match, or null when it touches non-prose markup. */
export function matchRange(block: LtBlock, match: LtMatch): { from: number; to: number } | null {
	const start = match.offset, end = match.offset + match.length;
	let from: number | null = null, to: number | null = null;
	for (const segment of block.segments) {
		const segmentEnd = segment.ltStart + segment.length;
		if (segmentEnd <= start || segment.ltStart >= end) continue;
		if (segment.pmStart === null) return null;
		if (start >= segment.ltStart && start < segmentEnd) from = segment.pmStart + start - segment.ltStart;
		if (end > segment.ltStart && end <= segmentEnd) to = segment.pmStart + end - segment.ltStart;
	}
	return from !== null && to !== null && to > from ? { from, to } : null;
}

function ignoreKey(match: LtMatch, text: string) {
	return `${match.ruleId}\u0000${text}`;
}

function loadIgnored(): Set<string> {
	try {
		const stored = JSON.parse(localStorage.getItem(IGNORED_STORAGE_KEY) ?? '[]');
		return new Set(Array.isArray(stored) ? stored.filter(v => typeof v === 'string') : []);
	} catch {
		return new Set();
	}
}

function saveIgnored(ignored: Set<string>) {
	try { localStorage.setItem(IGNORED_STORAGE_KEY, JSON.stringify([...ignored].slice(-2000))); } catch { /* storage full or unavailable */ }
}

function issueClass(issueType: string) {
	if (issueType === 'misspelling') return 'lt-spelling';
	if (issueType === 'style' || issueType === 'locale-violation' || issueType === 'register') return 'lt-style';
	return 'lt-grammar';
}

type DecorationSpec = { match: LtMatch; text: string };

export function buildDecorations(doc: ProseMirrorNode, cache: Map<string, LtMatch[]>, ignored: Set<string>): DecorationSet {
	const decorations: Decoration[] = [];
	for (const block of collectBlocks(doc)) {
		for (const match of cache.get(block.key) ?? []) {
			const range = matchRange(block, match);
			if (!range) continue;
			const text = doc.textBetween(range.from, range.to);
			if (ignored.has(ignoreKey(match, text))) continue;
			decorations.push(Decoration.inline(range.from, range.to,
				{ class: `lt-mark ${issueClass(match.issueType)}` },
				{ match, text } satisfies DecorationSpec));
		}
	}
	return DecorationSet.create(doc, decorations);
}

export type LanguageToolOptions = {
	check: (data: string) => Promise<LtMatch[]>;
	labels: { ignore: string; noSuggestions: string };
	onError?: (message: string) => void;
	debounceMs?: number;
};

export const languageToolKey = new PluginKey<DecorationSet>('languagetool');

export const LanguageTool = Extension.create<LanguageToolOptions>({
	name: 'languagetool',

	addOptions() {
		return {
			check: async () => [],
			labels: { ignore: 'Ignore', noSuggestions: 'No suggestions' },
			onError: undefined,
			debounceMs: 1200,
		};
	},

	addProseMirrorPlugins() {
		const options = this.options;
		const cache = new Map<string, LtMatch[]>();
		const ignored = loadIgnored();
		let popover: HTMLElement | null = null;

		const closePopover = () => { popover?.remove(); popover = null; };

		const refresh = (view: EditorView) => {
			if (view.isDestroyed) return;
			const decorations = buildDecorations(view.state.doc, cache, ignored);
			view.dispatch(view.state.tr.setMeta(languageToolKey, decorations).setMeta('addToHistory', false));
		};

		const openPopover = (view: EditorView, from: number, to: number, spec: DecorationSpec) => {
			closePopover();
			const { match, text } = spec;
			const el = document.createElement('div');
			el.className = 'lt-popover';
			el.setAttribute('role', 'dialog');
			const message = document.createElement('p');
			message.className = 'lt-message';
			message.textContent = match.message;
			el.append(message);
			const actions = document.createElement('div');
			actions.className = 'lt-actions';
			for (const replacement of match.replacements) {
				const button = document.createElement('button');
				button.type = 'button';
				button.className = 'lt-replacement';
				button.textContent = replacement || '␣';
				button.addEventListener('click', () => {
					closePopover();
					// Apply only if the flagged text is still there.
					if (view.state.doc.textBetween(from, to) !== text) return;
					view.dispatch(view.state.tr.insertText(replacement, from, to));
					view.focus();
				});
				actions.append(button);
			}
			if (!match.replacements.length) {
				const none = document.createElement('span');
				none.className = 'lt-none';
				none.textContent = options.labels.noSuggestions;
				actions.append(none);
			}
			const ignore = document.createElement('button');
			ignore.type = 'button';
			ignore.className = 'lt-ignore';
			ignore.textContent = options.labels.ignore;
			ignore.addEventListener('click', () => {
				closePopover();
				ignored.add(ignoreKey(match, text));
				saveIgnored(ignored);
				refresh(view);
				view.focus();
			});
			actions.append(ignore);
			el.append(actions);
			document.body.append(el);
			const coords = view.coordsAtPos(from);
			const width = el.offsetWidth;
			el.style.left = `${Math.max(8, Math.min(coords.left, window.innerWidth - width - 8))}px`;
			el.style.top = `${coords.bottom + 6}px`;
			popover = el;
		};

		return [
			new Plugin<DecorationSet>({
				key: languageToolKey,
				state: {
					init: () => DecorationSet.empty,
					apply(tr, decorations) {
						const next = tr.getMeta(languageToolKey) as DecorationSet | undefined;
						if (next) return next;
						if (!tr.docChanged) return decorations;
						let mapped = decorations.map(tr.mapping, tr.doc);
						// Edited text loses its marks at once; the next check restores real ones.
						tr.mapping.maps.forEach((map, index) => {
							map.forEach((_oldStart, _oldEnd, newStart, newEnd) => {
								const rest = tr.mapping.slice(index + 1);
								const from = rest.map(newStart, -1), to = rest.map(newEnd, 1);
								mapped = mapped.remove(mapped.find(Math.max(0, from - 1), to + 1));
							});
						});
						return mapped;
					},
				},
				props: {
					decorations: (state: EditorState) => languageToolKey.getState(state),
					handleClick(view, pos) {
						const found = languageToolKey.getState(view.state)?.find(pos, pos) ?? [];
						const decoration = found.find(d => d.from <= pos && d.to >= pos);
						if (!decoration) { closePopover(); return false; }
						openPopover(view, decoration.from, decoration.to, decoration.spec as DecorationSpec);
						return false;
					},
					handleKeyDown(_view, event) {
						if (event.key === 'Escape' && popover) { closePopover(); return true; }
						return false;
					},
				},
				view(view) {
					let timer: ReturnType<typeof setTimeout> | null = null;
					let running = false, again = false, lastError = '';

					const run = async () => {
						if (running) { again = true; return; }
						running = true;
						try {
							const seen = new Set<string>();
							const pending = collectBlocks(view.state.doc).filter(block => {
								if (cache.has(block.key) || seen.has(block.key)) return false;
								seen.add(block.key);
								return true;
							});
							for (const batch of buildBatches(pending)) {
								if (view.isDestroyed) return;
								const matches = await options.check(batch.data);
								splitMatches(matches, batch.starts, batch.blocks)
									.forEach((blockMatches, i) => cache.set(batch.blocks[i].key, blockMatches));
							}
							lastError = '';
							refresh(view);
						} catch (error) {
							const message = String(error);
							// Report each distinct failure once instead of on every pause in typing.
							if (message !== lastError) options.onError?.(message);
							lastError = message;
						} finally {
							running = false;
							if (again) { again = false; schedule(options.debounceMs ?? 1200); }
						}
					};

					const schedule = (delay: number) => {
						if (timer) clearTimeout(timer);
						timer = setTimeout(() => { timer = null; void run(); }, delay);
					};

					const onOutside = (event: MouseEvent) => {
						if (popover && !popover.contains(event.target as Node) && !view.dom.contains(event.target as Node)) closePopover();
					};
					document.addEventListener('mousedown', onOutside, true);
					schedule(300);

					return {
						update(view, previous) {
							if (!view.state.doc.eq(previous.doc)) {
								closePopover();
								schedule(options.debounceMs ?? 1200);
							}
						},
						destroy() {
							if (timer) clearTimeout(timer);
							closePopover();
							document.removeEventListener('mousedown', onOutside, true);
						},
					};
				},
			}),
		];
	},
});
