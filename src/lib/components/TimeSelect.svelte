<script lang="ts">
	import { tick, untrack } from 'svelte';

	// Time entry that relies on no native picker: native `type="time"`,
	// `datetime-local` and `<select>` popups are unusable or unthemed in
	// WebKitGTK (the Linux webview). Type a time (`9:30`, `930`, `21`) or pick
	// one from the list drawn here in the DOM.
	let { value = $bindable(''), allowEmpty = false, label = '', title = '', class: className = '' }: {
		/** `HH:MM` (24h), or '' when empty/invalid. */
		value?: string;
		allowEmpty?: boolean;
		label?: string;
		title?: string;
		class?: string;
	} = $props();

	const pad = (n: number) => String(n).padStart(2, '0');
	const SLOTS = Array.from({ length: 96 }, (_, i) => `${pad(Math.floor(i / 4))}:${pad((i % 4) * 15)}`);

	function parse(raw: string): string | null {
		const s = raw.trim().replace(/\s+/g, '');
		if (!s) return null;
		let h: number, m: number;
		let match = s.match(/^(\d{1,2})(?:[:.hH](\d{1,2})?)?$/);
		if (match) {
			h = Number(match[1]);
			m = match[2] ? Number(match[2]) : 0;
		} else if ((match = s.match(/^(\d{1,2})(\d{2})$/))) {
			h = Number(match[1]);
			m = Number(match[2]);
		} else return null;
		if (h > 23 || m > 59) return null;
		return `${pad(h)}:${pad(m)}`;
	}

	let text = $state(untrack(() => value));
	let open = $state(false);
	let root: HTMLSpanElement;
	let list = $state<HTMLDivElement>();
	const invalid = $derived(text.trim() !== '' && parse(text) === null);

	// Follow outside changes to `value` without clobbering what is being typed.
	$effect(() => {
		const v = value;
		if (untrack(() => parse(text) ?? '') !== v) text = v;
	});

	function onInput(raw: string) {
		text = raw;
		value = parse(raw) ?? '';
	}

	function step(delta: number) {
		const base = value || '09:00';
		const total = (Number(base.slice(0, 2)) * 60 + Number(base.slice(3, 5)) + delta + 1440) % 1440;
		value = text = `${pad(Math.floor(total / 60))}:${pad(total % 60)}`;
	}

	async function toggle() {
		open = !open;
		if (!open) return;
		await tick();
		const target = value || '09:00';
		const idx = SLOTS.findIndex((s) => s >= target);
		const el = list?.children[Math.max(0, idx)] as HTMLElement | undefined;
		if (el && list) list.scrollTop = el.offsetTop - list.clientHeight / 2 + el.offsetHeight / 2;
	}

	function pick(slot: string) {
		value = text = slot;
		open = false;
	}

	function onWindowPointerDown(e: PointerEvent) {
		if (open && !root.contains(e.target as Node)) open = false;
	}
</script>

<svelte:window onpointerdown={onWindowPointerDown} />

<span class="time-select {className}" bind:this={root} {title}>
	<input
		type="text"
		inputmode="numeric"
		autocomplete="off"
		spellcheck="false"
		placeholder={allowEmpty ? '--:--' : 'HH:MM'}
		aria-label={label}
		class:invalid
		value={text}
		oninput={(e) => onInput(e.currentTarget.value)}
		onblur={() => { if (!invalid) text = value; }}
		onkeydown={(e) => {
			if (e.key === 'ArrowUp') { e.preventDefault(); step(e.shiftKey ? 60 : 5); }
			else if (e.key === 'ArrowDown') { e.preventDefault(); step(e.shiftKey ? -60 : -5); }
			else if (e.key === 'Escape' && open) { e.stopPropagation(); open = false; }
		}}
	/>
	<button type="button" class="toggle" tabindex="-1" aria-label={label} aria-expanded={open} onclick={toggle}>
		<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9"/><polyline points="12 7 12 12 15 14"/></svg>
	</button>
	{#if open}
		<div class="time-list" role="listbox" aria-label={label} bind:this={list}>
			{#if allowEmpty}
				<button type="button" role="option" aria-selected={!value} class:selected={!value} onmousedown={(e) => e.preventDefault()} onclick={() => pick('')}>--:--</button>
			{/if}
			{#each SLOTS as slot}
				<button type="button" role="option" aria-selected={slot === value} class:selected={slot === value} onmousedown={(e) => e.preventDefault()} onclick={() => pick(slot)}>{slot}</button>
			{/each}
		</div>
	{/if}
</span>

<style>
	.time-select {
		position: relative;
		display: inline-flex;
		align-items: stretch;
		border: 1px solid var(--border-color);
		border-radius: 4px;
		background: var(--bg-secondary);
		color: var(--text-primary);
	}
	.time-select:focus-within { border-color: var(--accent); }
	input {
		appearance: none;
		width: 5.5ch;
		min-width: 0;
		padding: 3px 2px 3px 6px;
		border: none;
		outline: none;
		background: transparent;
		color: inherit;
		font: inherit;
		font-variant-numeric: tabular-nums;
	}
	input::placeholder { color: var(--text-secondary, inherit); opacity: 0.6; }
	input.invalid { color: var(--danger, #e5484d); }
	.toggle {
		appearance: none;
		display: inline-flex;
		align-items: center;
		padding: 0 6px;
		border: none;
		background: transparent;
		color: var(--text-secondary, inherit);
		cursor: pointer;
	}
	.toggle:hover { color: var(--text-primary); }
	.time-list {
		position: absolute;
		top: calc(100% + 3px);
		left: -1px;
		z-index: 60;
		max-height: 200px;
		overflow-y: auto;
		min-width: calc(100% + 2px);
		padding: 4px;
		background: var(--bg-primary);
		border: 1px solid var(--border-color);
		border-radius: 6px;
		box-shadow: 0 4px 14px rgba(0, 0, 0, 0.25);
	}
	.time-list button {
		display: block;
		width: 100%;
		padding: 3px 8px;
		border: none;
		border-radius: 4px;
		background: none;
		color: var(--text-primary);
		font: inherit;
		font-variant-numeric: tabular-nums;
		text-align: left;
		cursor: pointer;
	}
	.time-list button:hover { background: var(--bg-hover, var(--bg-secondary)); }
	.time-list button.selected { background: var(--accent); color: white; }
</style>
