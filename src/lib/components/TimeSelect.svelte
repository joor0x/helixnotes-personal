<script lang="ts">
	// Hour/minute picker built from plain selects. Native `type="time"` and
	// `type="datetime-local"` inputs have no usable time picker in WebKitGTK
	// (the Linux webview), so time entry goes through this control everywhere.
	let { value = $bindable(''), allowEmpty = false, label = '', title = '', class: className = '' }: {
		/** `HH:MM` (24h), or '' when `allowEmpty` and no time is set. */
		value?: string;
		allowEmpty?: boolean;
		label?: string;
		title?: string;
		class?: string;
	} = $props();

	const pad = (n: number) => String(n).padStart(2, '0');
	const HOURS = Array.from({ length: 24 }, (_, i) => pad(i));
	const BASE_MINUTES = Array.from({ length: 12 }, (_, i) => pad(i * 5));

	const hour = $derived(/^\d{2}:\d{2}/.test(value) ? value.slice(0, 2) : '');
	const minute = $derived(/^\d{2}:\d{2}/.test(value) ? value.slice(3, 5) : '');
	// Keep an existing off-grid minute (e.g. 10:07) selectable.
	const minutes = $derived(minute && !BASE_MINUTES.includes(minute) ? [...BASE_MINUTES, minute].sort() : BASE_MINUTES);

	function setHour(h: string) {
		if (!h) { value = ''; return; }
		value = `${h}:${minute || '00'}`;
	}

	function setMinute(m: string) {
		value = `${hour || '09'}:${m}`;
	}
</script>

<span class="time-select {className}" role="group" aria-label={label} {title}>
	<select aria-label={label ? `${label} (h)` : undefined} value={hour} onchange={(e) => setHour(e.currentTarget.value)}>
		{#if allowEmpty || !hour}<option value="">--</option>{/if}
		{#each HOURS as h}<option value={h}>{h}</option>{/each}
	</select>
	<span class="sep">:</span>
	<select aria-label={label ? `${label} (min)` : undefined} value={minute} disabled={allowEmpty && !hour} onchange={(e) => setMinute(e.currentTarget.value)}>
		{#if !minute}<option value="">--</option>{/if}
		{#each minutes as m}<option value={m}>{m}</option>{/each}
	</select>
</span>

<style>
	.time-select {
		display: inline-flex;
		align-items: center;
		gap: 2px;
	}
	.time-select select {
		font: inherit;
		font-size: inherit;
		padding: 2px 4px;
		border: 1px solid var(--border-color);
		border-radius: 4px;
		background: var(--bg-secondary);
		color: var(--text-primary);
		color-scheme: light dark;
		cursor: pointer;
	}
	.time-select select:disabled { opacity: 0.5; cursor: default; }
	.sep { color: var(--text-secondary, inherit); }
</style>
