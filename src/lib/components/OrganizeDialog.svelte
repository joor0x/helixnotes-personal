<script lang="ts">
	import { onDestroy } from 'svelte';
	import { listen } from '@tauri-apps/api/event';
	import { t as ui } from '$lib/i18n';
	import { appConfig } from '$lib/stores/app';
	import { organizePlan, organizeCancel, type OrganizeChange, type OrganizePlan } from '$lib/api';

	let { vault, folder, name, onclose, onapply }: {
		vault: string;
		/** Absolute path of the folder to organize; the vault path for unfiled notes. */
		folder: string;
		name: string;
		onclose: () => void;
		/** Applies the accepted suggestions; resolves to how many of them failed. */
		onapply: (changes: OrganizeChange[]) => Promise<number>;
	} = $props();

	const KEEP = 'keep';
	const TRASH = 'trash';
	const notebookChoice = (relativePath: string) => `nb:${relativePath}`;

	let phase = $state<'intro' | 'working' | 'review' | 'applying' | 'failed'>('intro');
	let error = $state('');
	let failed = $state(0);
	let progress = $state<{ stage: string; done: number; total: number } | null>(null);
	let plan = $state<OrganizePlan | null>(null);
	/** One entry per suggestion: KEEP, TRASH or a notebook choice. */
	let choices = $state<string[]>([]);
	let run = 0;
	let disposed = false;

	const destination = $derived.by(() => {
		const config = $appConfig;
		const provider = config?.ai_provider ?? '';
		const server = provider === 'ollama' ? config?.ollama_base_url || 'http://localhost:11434'
			: provider === 'openai_compatible' ? config?.openai_compatible_base_url || ''
			: '';
		return [provider + (server ? ` (${server})` : ''), config?.ai_model].filter(Boolean).join(' · ');
	});
	const moves = $derived(choices.filter((choice) => choice !== KEEP && choice !== TRASH).length);
	const trashed = $derived(choices.filter((choice) => choice === TRASH).length);

	const unlisten = listen<{ stage: string; done: number; total: number }>('organize-progress', (event) => {
		if (phase === 'working') progress = event.payload;
	});

	onDestroy(() => {
		disposed = true;
		if (phase === 'working') void organizeCancel();
		void unlisten.then((stop) => stop());
	});

	async function start() {
		const current = ++run;
		phase = 'working';
		error = '';
		progress = null;
		try {
			const result = await organizePlan(vault, folder);
			if (disposed || current !== run) return;
			plan = result;
			choices = result.suggestions.map((suggestion) =>
				suggestion.action === 'trash' ? TRASH
					: suggestion.action === 'move' && suggestion.notebook !== null ? notebookChoice(suggestion.notebook)
					: KEEP
			);
			phase = 'review';
		} catch (e) {
			if (disposed || current !== run) return;
			error = String(e);
			phase = 'intro';
		}
	}

	function close() {
		if (phase === 'applying') return;
		if (phase === 'working') {
			run++;
			void organizeCancel();
		}
		onclose();
	}

	async function apply() {
		if (!plan) return;
		const paths = new Map(plan.notebooks.map((notebook) => [notebookChoice(notebook.relative_path), notebook.path]));
		const changes: OrganizeChange[] = [];
		plan.suggestions.forEach((suggestion, index) => {
			const choice = choices[index];
			if (choice === TRASH) changes.push({ path: suggestion.path, dest: null });
			else if (paths.has(choice)) changes.push({ path: suggestion.path, dest: paths.get(choice)! });
		});
		phase = 'applying';
		failed = await onapply(changes);
		if (disposed) return;
		if (failed > 0) phase = 'failed';
		else onclose();
	}
</script>

<svelte:window onkeydown={(e) => { if (e.key === 'Escape') close(); }} />

<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
<div class="organize-overlay" onclick={(e) => { if (e.target === e.currentTarget) close(); }}>
	<div class="organize" role="dialog" aria-modal="true" aria-labelledby="organize-title">
		<h4 id="organize-title">{$ui('organize.title', { folder: name })}</h4>

		{#if phase === 'intro'}
			<p>{$ui('organize.intro')}</p>
			<p class="muted">{$ui('organize.sends', { destination })}</p>
			{#if error}<p class="error" role="alert">{error}</p>{/if}
			<div class="actions">
				<button class="secondary" onclick={close}>{$ui('ui.cancel')}</button>
				<button class="primary" onclick={start}>{error ? $ui('ui.retry') : $ui('organize.start')}</button>
			</div>
		{:else if phase === 'working'}
			<p class="status" role="status">
				<span class="spinner" aria-hidden="true"></span>
				{#if progress?.stage === 'notebooks'}
					{$ui('organize.describingNotebooks')}
				{:else if progress}
					{$ui('organize.readingNotes', { done: progress.done, total: progress.total })}
				{:else}
					{$ui('organize.preparing')}
				{/if}
			</p>
			<div class="actions">
				<button class="secondary" onclick={close}>{$ui('ui.cancel')}</button>
			</div>
		{:else if phase === 'failed'}
			<p class="error" role="alert">{$ui('organize.failed', { count: failed })}</p>
			<div class="actions">
				<button class="primary" onclick={onclose}>{$ui('ui.close')}</button>
			</div>
		{:else if plan}
			{#if plan.suggestions.length === 0}
				<p>{$ui('organize.nothing')}</p>
			{:else}
				<p class="muted">{$ui('organize.summary', { moves, trash: trashed, stay: choices.length - moves - trashed })}</p>
				<ul class="rows">
					{#each plan.suggestions as suggestion, index (suggestion.path)}
						<li class:idle={choices[index] === KEEP}>
							<span class="note-title" title={suggestion.title}>
								{suggestion.title}
								{#if suggestion.empty}<span class="badge">{$ui('organize.empty')}</span>{/if}
							</span>
							<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12h14" /><path d="m13 6 6 6-6 6" /></svg>
							<select
								bind:value={choices[index]}
								class:trash={choices[index] === TRASH}
								disabled={phase === 'applying'}
								aria-label={$ui('organize.destinationFor', { title: suggestion.title })}
							>
								<option value={KEEP}>{$ui('organize.leave')}</option>
								<option value={TRASH}>{$ui('ui.trash')}</option>
								{#each plan.notebooks as notebook (notebook.path)}
									<option value={notebookChoice(notebook.relative_path)}>{notebook.relative_path}</option>
								{/each}
							</select>
						</li>
					{/each}
				</ul>
				{#if plan.unanswered > 0}<p class="muted">{$ui('organize.unanswered', { count: plan.unanswered })}</p>{/if}
				{#if plan.remaining > 0}<p class="muted">{$ui('organize.remaining', { count: plan.remaining })}</p>{/if}
			{/if}
			<div class="actions">
				<button class="secondary" disabled={phase === 'applying'} onclick={close}>{$ui('ui.cancel')}</button>
				<button class="primary" disabled={phase === 'applying' || moves + trashed === 0} onclick={apply}>
					{$ui('organize.apply', { count: moves + trashed })}
				</button>
			</div>
		{/if}
	</div>
</div>

<style>
	.organize-overlay {
		position: fixed;
		inset: 0;
		background: rgba(0, 0, 0, 0.4);
		display: flex;
		align-items: center;
		justify-content: center;
		z-index: 1000;
	}

	.organize {
		display: flex;
		flex-direction: column;
		width: min(620px, calc(100vw - 32px));
		max-height: calc(100vh - 64px);
		background: var(--bg-primary);
		border: 1px solid var(--border-color);
		border-radius: 10px;
		padding: 20px 24px;
		box-shadow: 0 8px 32px rgba(0, 0, 0, 0.25);
	}

	h4 {
		margin: 0 0 10px;
		font-size: 14px;
		color: var(--text-primary);
		overflow-wrap: anywhere;
	}

	p {
		margin: 0 0 10px;
		font-size: 12.5px;
		line-height: 1.45;
		color: var(--text-primary);
	}

	p.muted {
		color: var(--text-secondary);
		overflow-wrap: anywhere;
	}

	p.error {
		color: var(--danger, #e53e3e);
		overflow-wrap: anywhere;
	}

	.status {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 12px 0;
		font-variant-numeric: tabular-nums;
	}

	.spinner {
		width: 14px;
		height: 14px;
		flex-shrink: 0;
		border: 2px solid var(--border-color);
		border-top-color: var(--accent, var(--text-primary));
		border-radius: 50%;
		animation: organize-spin 0.8s linear infinite;
	}

	@keyframes organize-spin {
		to { transform: rotate(360deg); }
	}

	@media (prefers-reduced-motion: reduce) {
		.spinner { animation: none; }
	}

	.rows {
		list-style: none;
		margin: 0 0 10px;
		padding: 0;
		min-height: 0;
		overflow-y: auto;
		border: 1px solid var(--border-color);
		border-radius: 8px;
	}

	.rows li {
		display: grid;
		grid-template-columns: minmax(0, 1fr) 14px minmax(0, 220px);
		align-items: center;
		gap: 10px;
		padding: 6px 10px;
		font-size: 12.5px;
		color: var(--text-primary);
	}

	.rows li + li {
		border-top: 1px solid var(--border-color);
	}

	.rows li svg {
		color: var(--text-secondary);
	}

	.rows li.idle .note-title,
	.rows li.idle svg {
		opacity: 0.55;
	}

	.note-title {
		overflow: hidden;
		white-space: nowrap;
		text-overflow: ellipsis;
	}

	.badge {
		margin-left: 6px;
		padding: 1px 6px;
		border-radius: 999px;
		background: var(--bg-secondary);
		color: var(--text-secondary);
		font-size: 11px;
	}

	select {
		width: 100%;
		min-width: 0;
		padding: 4px 6px;
		border: 1px solid var(--border-color);
		border-radius: 6px;
		background: var(--bg-secondary);
		color: var(--text-primary);
		font-size: 12px;
	}

	select.trash {
		color: var(--danger, #e53e3e);
	}

	.actions {
		display: flex;
		gap: 8px;
		justify-content: flex-end;
		margin-top: 6px;
	}

	.actions button {
		padding: 6px 14px;
		border-radius: 6px;
		font-size: 12px;
		cursor: pointer;
	}

	.actions button:disabled {
		opacity: 0.5;
		cursor: default;
	}

	.secondary {
		border: 1px solid var(--border-color);
		background: var(--bg-secondary);
		color: var(--text-primary);
	}

	.secondary:hover:not(:disabled) {
		background: var(--bg-hover);
	}

	.primary {
		border: none;
		background: var(--accent, #5b6abf);
		color: white;
		font-weight: 600;
	}

	.primary:hover:not(:disabled) {
		opacity: 0.9;
	}
</style>
