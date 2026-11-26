<script lang="ts">
	import { t as ui, locale } from '$lib/i18n';
	import { onMount } from 'svelte';
	import { listen } from '@tauri-apps/api/event';
	import { getHabits } from '$lib/api';
	import { debounce } from '$lib/utils/debounce';
	import { appConfig, activeVaultConfig } from '$lib/stores/app';
	import type { Habit, FileEvent } from '$lib/types';
	import { isCurrentVaultEvent } from '$lib/utils/note-list-updates';
	import { formatLocalDate } from '$lib/utils/time';

	let { compact = false, onOpen = (_path: string) => {}, onToggle = async (_path: string, _date: string): Promise<string[] | null> => null, onCreate = null }: {
		/** Narrow layout for the notes pane on phones and tablets. */
		compact?: boolean;
		onOpen?: (path: string) => void;
		/** Marks or unmarks a day; resolves to the habit's days, or null when nothing changed. */
		onToggle?: (path: string, date: string) => Promise<string[] | null>;
		onCreate?: (() => void) | null;
	} = $props();

	const MAX_DAYS = 21;
	const MIN_DAYS = 7;

	let habits = $state<Habit[]>([]);
	let loading = $state(true);
	let today = $state(formatLocalDate(new Date()));
	/** Days back from today of the last column shown. */
	let offset = $state(0);
	let width = $state(0);
	let busy = $state<string | null>(null);
	let disposed = false;
	let request = 0;

	const folder = $derived(activeVaultConfig($appConfig)?.habits_folder ?? null);
	const nameWidth = $derived(compact ? 110 : 190);
	const cellWidth = $derived(compact ? 30 : 32);
	// Room left for day columns: the width minus the name column, the padding and (desktop) the streak column.
	const dayCount = $derived(
		width > 0
			? Math.max(MIN_DAYS, Math.min(MAX_DAYS, Math.floor((width - nameWidth - (compact ? 22 : 122)) / cellWidth)))
			: MAX_DAYS
	);

	function addDays(date: string, amount: number): string {
		const [year, month, day] = date.split('-').map(Number);
		return formatLocalDate(new Date(year, month - 1, day + amount));
	}

	const days = $derived.by(() => {
		const weekday = new Intl.DateTimeFormat($locale, { weekday: 'narrow' });
		const last = addDays(today, -offset);
		return Array.from({ length: dayCount }, (_, index) => {
			const date = addDays(last, index - dayCount + 1);
			const [year, month, day] = date.split('-').map(Number);
			const local = new Date(year, month - 1, day);
			return {
				date,
				day,
				weekday: weekday.format(local),
				weekend: local.getDay() === 0 || local.getDay() === 6,
				today: date === today,
				future: date > today,
			};
		});
	});

	const rangeLabel = $derived.by(() => {
		if (days.length === 0) return '';
		const format = new Intl.DateTimeFormat($locale, { day: 'numeric', month: 'short', year: 'numeric' });
		const parse = (date: string) => {
			const [year, month, day] = date.split('-').map(Number);
			return new Date(year, month - 1, day);
		};
		return format.formatRange(parse(days[0].date), parse(days[days.length - 1].date));
	});

	/** Consecutive days done up to today; a day still open today does not break the run. */
	function streak(entries: string[]): number {
		const done = new Set(entries);
		let cursor = done.has(today) ? today : addDays(today, -1);
		let count = 0;
		while (done.has(cursor)) {
			count += 1;
			cursor = addDays(cursor, -1);
		}
		return count;
	}

	async function load() {
		if (disposed) return;
		const current = ++request;
		try {
			const loaded = await getHabits();
			if (disposed || current !== request) return;
			habits = loaded;
		} catch (error) {
			console.error('Failed to load habits:', error);
		} finally {
			if (!disposed && current === request) loading = false;
		}
	}

	const debouncedLoad = debounce(load, 300);

	async function toggle(habit: Habit, date: string) {
		const key = `${habit.path}|${date}`;
		if (busy || date > today) return;
		busy = key;
		try {
			const entries = await onToggle(habit.path, date);
			if (entries && !disposed) {
				request += 1;
				habits = habits.map((item) => (item.path === habit.path ? { ...item, entries } : item));
			}
		} catch (error) {
			console.error('Failed to update habit:', error);
			void load();
		} finally {
			busy = null;
		}
	}

	$effect(() => {
		// A different habits folder (or vault) is a different list.
		void folder;
		void $appConfig?.active_vault;
		loading = true;
		void load();
	});

	onMount(() => {
		let dayTimer: ReturnType<typeof setTimeout> | undefined;
		const refreshDay = () => {
			const now = new Date();
			today = formatLocalDate(now);
			clearTimeout(dayTimer);
			const nextDay = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
			dayTimer = setTimeout(refreshDay, nextDay.getTime() - now.getTime() + 1);
		};
		const resumeDay = () => {
			if (document.visibilityState === 'visible') refreshDay();
		};
		refreshDay();
		document.addEventListener('visibilitychange', resumeDay);
		let unlisten: (() => void) | null = null;
		listen<FileEvent>('file-changed', ({ payload }) => {
			if (!disposed && isCurrentVaultEvent(payload, $appConfig?.active_vault ?? null)) debouncedLoad();
		}).then((stop) => {
			if (disposed) stop();
			else unlisten = stop;
		});
		return () => {
			disposed = true;
			clearTimeout(dayTimer);
			document.removeEventListener('visibilitychange', resumeDay);
			unlisten?.();
		};
	});
</script>

<div class="habits-view" class:compact bind:clientWidth={width}>
	<div class="habits-head">
		{#if !compact}<h2>{$ui("habits.title")}</h2>{/if}
		<span class="habits-range">{rangeLabel}</span>
		<div class="habits-nav">
			<button class="habits-btn" onclick={() => (offset += dayCount)} title={$ui("habits.earlier")} aria-label={$ui("habits.earlier")}>
				<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
			</button>
			<button class="habits-btn text" disabled={offset === 0} onclick={() => (offset = 0)}>{$ui("ui.today")}</button>
			<button class="habits-btn" disabled={offset === 0} onclick={() => (offset = Math.max(0, offset - dayCount))} title={$ui("habits.later")} aria-label={$ui("habits.later")}>
				<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg>
			</button>
			{#if onCreate}
				<button class="habits-btn text" onclick={() => onCreate?.()}>{$ui("habits.new")}</button>
			{/if}
		</div>
	</div>

	{#if loading && habits.length === 0}
		<p class="habits-empty"></p>
	{:else if habits.length === 0}
		<p class="habits-empty">{$ui("habits.empty", { folder: folder ?? '' })}</p>
	{:else}
		<div class="habits-scroll">
			<table class="habits-grid" style:--habit-name-width="{nameWidth}px" style:--habit-cell-width="{cellWidth}px">
				<thead>
					<tr>
						<th class="habit-name"></th>
						{#each days as day (day.date)}
							<th class="habit-day" class:weekend={day.weekend} class:today={day.today} title={day.date}>
								<span class="habit-weekday">{day.weekday}</span>
								<span>{day.day}</span>
							</th>
						{/each}
						{#if !compact}<th class="habit-streak" title={$ui("habits.streak")}>{$ui("habits.streak")}</th>{/if}
					</tr>
				</thead>
				<tbody>
					{#each habits as habit (habit.path)}
						{@const done = new Set(habit.entries)}
						<tr>
							<th class="habit-name" scope="row">
								<button class="habit-link" onclick={() => onOpen(habit.path)} title={habit.title}>{habit.title}</button>
							</th>
							{#each days as day (day.date)}
								<td class="habit-cell" class:weekend={day.weekend}>
									<button
										class="habit-tick"
										class:done={done.has(day.date)}
										disabled={day.future}
										aria-pressed={done.has(day.date)}
										aria-label={`${habit.title} · ${day.date}`}
										onclick={() => toggle(habit, day.date)}
									></button>
								</td>
							{/each}
							{#if !compact}<td class="habit-streak">{streak(habit.entries) || ''}</td>{/if}
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{/if}
</div>

<style>
	.habits-view {
		display: flex;
		flex-direction: column;
		height: 100%;
		min-width: 0;
		padding: 28px 32px;
		box-sizing: border-box;
		overflow: hidden;
		color: var(--text-primary);
	}
	.habits-view.compact {
		height: auto;
		padding: 10px 10px 12px;
		border-bottom: 1px solid var(--border-color);
	}
	.habits-head {
		display: flex;
		align-items: center;
		gap: 12px;
		flex-wrap: wrap;
		margin-bottom: 16px;
		flex-shrink: 0;
	}
	.habits-view.compact .habits-head {
		margin-bottom: 8px;
	}
	.habits-head h2 {
		margin: 0;
		font-size: 22px;
		font-weight: 600;
	}
	.habits-range {
		color: var(--text-tertiary);
		font-size: 13px;
	}
	.habits-nav {
		display: flex;
		gap: 4px;
		margin-left: auto;
	}
	.habits-btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		min-width: 26px;
		height: 26px;
		padding: 0 5px;
		border: 1px solid var(--border-color);
		border-radius: 5px;
		background: var(--bg-secondary);
		color: var(--text-secondary);
		font-size: 12px;
		cursor: pointer;
	}
	.habits-btn.text {
		padding: 0 9px;
	}
	.habits-btn:hover:not(:disabled) {
		background: var(--bg-hover);
		color: var(--text-primary);
	}
	.habits-btn:disabled {
		opacity: 0.45;
		cursor: default;
	}
	.habits-view.compact .habits-btn {
		min-width: 36px;
		height: 36px;
	}
	.habits-empty {
		margin: 24px 0;
		color: var(--text-tertiary);
		font-size: 14px;
	}
	.habits-scroll {
		overflow: auto;
		min-height: 0;
	}
	.habits-grid {
		border-collapse: collapse;
		table-layout: fixed;
		font-size: 13px;
	}
	.habits-grid th,
	.habits-grid td {
		padding: 0;
		border: 1px solid var(--border-light, var(--border-color));
		height: var(--habit-cell-width);
	}
	.habits-grid thead th {
		border-top: none;
	}
	.habit-name {
		width: var(--habit-name-width);
		min-width: var(--habit-name-width);
		max-width: var(--habit-name-width);
		border-left: none !important;
		text-align: left;
		font-weight: 400;
		position: sticky;
		left: 0;
		z-index: 1;
		background: var(--bg-primary);
	}
	.habit-link {
		display: block;
		width: 100%;
		padding: 0 10px 0 2px;
		border: none;
		background: none;
		color: var(--accent);
		font: inherit;
		text-align: left;
		text-decoration: underline;
		text-underline-offset: 2px;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		cursor: pointer;
	}
	.habit-day {
		width: var(--habit-cell-width);
		min-width: var(--habit-cell-width);
		padding: 4px 0 !important;
		height: auto !important;
		color: var(--text-secondary);
		font-weight: 400;
		font-variant-numeric: tabular-nums;
		text-align: center;
	}
	.habit-day span {
		display: block;
	}
	.habit-weekday {
		color: var(--text-tertiary);
		font-size: 10px;
		text-transform: uppercase;
	}
	.habit-day.today {
		color: var(--accent);
		font-weight: 700;
	}
	.weekend {
		background: color-mix(in srgb, var(--text-primary) 5%, transparent);
	}
	.habit-tick {
		display: block;
		width: 100%;
		height: var(--habit-cell-width);
		padding: 0;
		border: none;
		background: none;
		cursor: pointer;
	}
	.habit-tick:hover:not(:disabled):not(.done) {
		background: color-mix(in srgb, var(--accent) 18%, transparent);
	}
	.habit-tick.done {
		background: var(--accent);
		box-shadow: inset 0 0 0 2px var(--bg-primary);
		border-radius: 6px;
	}
	.habit-tick:disabled {
		cursor: default;
	}
	.habit-tick:focus-visible,
	.habit-link:focus-visible {
		outline: 2px solid var(--accent);
		outline-offset: -2px;
	}
	.habit-streak {
		width: 56px;
		border-right: none !important;
		color: var(--text-tertiary);
		font-size: 11px;
		font-weight: 400;
		font-variant-numeric: tabular-nums;
		text-align: center;
	}
	td.habit-streak {
		color: var(--text-secondary);
		font-size: 13px;
	}
</style>
