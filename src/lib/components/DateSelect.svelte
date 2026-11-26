<script lang="ts">
	import { onMount, tick, untrack } from 'svelte';
	import { locale } from '$lib/i18n';

	// Date picker drawn in the DOM. The native `type="date"` calendar is a GTK
	// popover in WebKitGTK (Linux): unthemed, and it stays open after a day is
	// chosen because the page cannot see or close it. Here one click on a day
	// sets the date and closes the calendar.
	let { value = $bindable(''), open = $bindable(false), label = '', autofocus = false, class: className = '', onpick }: {
		/** `YYYY-MM-DD`, or '' when unset. */
		value?: string;
		open?: boolean;
		label?: string;
		/** Focus the field (or the calendar, when it starts open) on mount. */
		autofocus?: boolean;
		class?: string;
		/** Called after a day is clicked, with the new value. */
		onpick?: (value: string) => void;
	} = $props();

	const pad = (n: number) => String(n).padStart(2, '0');
	const iso = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
	const fromIso = (s: string) => {
		const m = s.match(/^(\d{4})-(\d{2})-(\d{2})$/);
		return m ? new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3])) : null;
	};
	const todayIso = iso(new Date());

	let root: HTMLSpanElement;
	let trigger: HTMLButtonElement;
	let grid = $state<HTMLDivElement>();
	/** Day with keyboard focus; also decides which month is shown. */
	let cursor = $state(untrack(() => fromIso(value) ?? new Date()));

	const firstDay = $derived.by(() => {
		try {
			const info = (new Intl.Locale($locale) as Intl.Locale & { getWeekInfo?: () => { firstDay: number }; weekInfo?: { firstDay: number } });
			return ((info.getWeekInfo?.() ?? info.weekInfo)?.firstDay ?? 1) % 7;
		} catch { return 1; }
	});
	const weekdays = $derived.by(() => {
		const fmt = new Intl.DateTimeFormat($locale, { weekday: 'narrow' });
		// 2024-01-07 is a Sunday.
		return Array.from({ length: 7 }, (_, i) => fmt.format(new Date(2024, 0, 7 + ((firstDay + i) % 7))));
	});
	const monthTitle = $derived(new Intl.DateTimeFormat($locale, { month: 'long', year: 'numeric' }).format(cursor));
	const display = $derived.by(() => {
		const d = fromIso(value);
		return d ? new Intl.DateTimeFormat($locale, { dateStyle: 'medium' }).format(d) : '';
	});
	const days = $derived.by(() => {
		const first = new Date(cursor.getFullYear(), cursor.getMonth(), 1);
		const offset = (first.getDay() - firstDay + 7) % 7;
		return Array.from({ length: 42 }, (_, i) => {
			const d = new Date(first.getFullYear(), first.getMonth(), 1 - offset + i);
			return { iso: iso(d), day: d.getDate(), inMonth: d.getMonth() === first.getMonth() };
		});
	});

	async function focusCursor() {
		await tick();
		(grid?.querySelector(`[data-day="${iso(cursor)}"]`) as HTMLElement | null)?.focus();
	}

	function show() {
		cursor = fromIso(value) ?? new Date();
		open = true;
		focusCursor();
	}

	function close(refocus = true) {
		open = false;
		if (refocus) tick().then(() => trigger?.focus());
	}

	function pick(dayIso: string) {
		value = dayIso;
		close();
		onpick?.(dayIso);
	}

	function moveMonth(delta: number) {
		const target = new Date(cursor.getFullYear(), cursor.getMonth() + delta, 1);
		const last = new Date(target.getFullYear(), target.getMonth() + 1, 0).getDate();
		cursor = new Date(target.getFullYear(), target.getMonth(), Math.min(cursor.getDate(), last));
	}

	function onGridKeydown(e: KeyboardEvent) {
		const moves: Record<string, number> = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -7, ArrowDown: 7 };
		if (e.key in moves) {
			e.preventDefault();
			cursor = new Date(cursor.getFullYear(), cursor.getMonth(), cursor.getDate() + moves[e.key]);
			focusCursor();
		} else if (e.key === 'PageUp' || e.key === 'PageDown') {
			e.preventDefault();
			moveMonth(e.key === 'PageUp' ? -1 : 1);
			focusCursor();
		}
	}

	function onWindowPointerDown(e: PointerEvent) {
		if (open && !root.contains(e.target as Node)) close(false);
	}

	onMount(() => {
		if (!autofocus) return;
		if (open) focusCursor();
		else trigger.focus();
	});
</script>

<svelte:window onpointerdown={onWindowPointerDown} />

<span class="date-select {className}" bind:this={root}>
	<button
		type="button"
		class="field"
		class:empty={!display}
		aria-label={label}
		aria-haspopup="dialog"
		aria-expanded={open}
		bind:this={trigger}
		onclick={() => (open ? close() : show())}
		onkeydown={(e) => { if (e.key === 'ArrowDown' && !open) { e.preventDefault(); show(); } }}
	>
		<span>{display || '--/--/----'}</span>
		<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="5" width="18" height="16" rx="2"/><line x1="16" y1="3" x2="16" y2="7"/><line x1="8" y1="3" x2="8" y2="7"/><line x1="3" y1="10" x2="21" y2="10"/></svg>
	</button>
	{#if open}
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<div
			class="calendar"
			role="dialog"
			aria-label={label}
			tabindex="-1"
			onkeydown={(e) => { if (e.key === 'Escape') { e.stopPropagation(); close(); } }}
		>
			<div class="head">
				<button type="button" class="nav" aria-label="‹" onclick={() => moveMonth(-1)}>‹</button>
				<span class="title">{monthTitle}</span>
				<button type="button" class="nav" aria-label="›" onclick={() => moveMonth(1)}>›</button>
			</div>
			<div class="weekdays" aria-hidden="true">
				{#each weekdays as w}<span>{w}</span>{/each}
			</div>
			<!-- svelte-ignore a11y_no_static_element_interactions -->
			<div class="days" bind:this={grid} onkeydown={onGridKeydown}>
				{#each days as d (d.iso)}
					<button
						type="button"
						data-day={d.iso}
						class:out={!d.inMonth}
						class:today={d.iso === todayIso}
						class:selected={d.iso === value}
						tabindex={d.iso === iso(cursor) ? 0 : -1}
						onclick={() => pick(d.iso)}
					>{d.day}</button>
				{/each}
			</div>
			<div class="foot">
				<button type="button" class="nav today-btn" onclick={() => pick(todayIso)}>{new Intl.RelativeTimeFormat($locale, { numeric: 'auto' }).format(0, 'day')}</button>
			</div>
		</div>
	{/if}
</span>

<style>
	.date-select {
		position: relative;
		display: inline-flex;
	}
	.field {
		appearance: none;
		display: inline-flex;
		align-items: center;
		justify-content: space-between;
		gap: 8px;
		padding: 3px 6px;
		border: 1px solid var(--border-color);
		border-radius: 4px;
		background: var(--bg-secondary);
		color: var(--text-primary);
		font: inherit;
		font-variant-numeric: tabular-nums;
		cursor: pointer;
		white-space: nowrap;
	}
	.field:focus-visible, .field[aria-expanded='true'] { outline: none; border-color: var(--accent); }
	.field.empty span { opacity: 0.6; }
	.field svg { color: var(--text-secondary, inherit); flex-shrink: 0; }
	.calendar {
		position: absolute;
		top: calc(100% + 4px);
		left: 0;
		z-index: 60;
		width: 224px;
		padding: 8px;
		background: var(--bg-primary);
		color: var(--text-primary);
		border: 1px solid var(--border-color);
		border-radius: 8px;
		box-shadow: 0 6px 20px rgba(0, 0, 0, 0.3);
		font-size: 12px;
	}
	.head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		margin-bottom: 6px;
	}
	.title { font-weight: 600; text-transform: capitalize; }
	.nav {
		appearance: none;
		border: none;
		background: none;
		color: var(--text-primary);
		font: inherit;
		font-size: 14px;
		padding: 2px 8px;
		border-radius: 4px;
		cursor: pointer;
	}
	.nav:hover { background: var(--bg-hover, var(--bg-secondary)); }
	.weekdays, .days {
		display: grid;
		grid-template-columns: repeat(7, 1fr);
		gap: 2px;
		text-align: center;
	}
	.weekdays span { color: var(--text-secondary, inherit); opacity: 0.8; padding: 2px 0; text-transform: uppercase; font-size: 10px; }
	.days button {
		appearance: none;
		border: none;
		border-radius: 4px;
		background: none;
		color: var(--text-primary);
		font: inherit;
		font-variant-numeric: tabular-nums;
		padding: 5px 0;
		cursor: pointer;
	}
	.days button:hover { background: var(--bg-hover, var(--bg-secondary)); }
	.days button:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
	.days button.out { opacity: 0.4; }
	.days button.today { box-shadow: inset 0 0 0 1px var(--accent); }
	.days button.selected { background: var(--accent); color: white; opacity: 1; }
	.foot { display: flex; justify-content: center; margin-top: 6px; }
	.today-btn { font-size: 12px; color: var(--accent); text-transform: capitalize; }
</style>
