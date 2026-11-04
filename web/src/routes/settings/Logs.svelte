<script lang="ts">
	import { api, type LogLevel, type LogLine } from '#lib/api/index.ts';
	import StateChip, { type ChipTone } from '#lib/components/StateChip.svelte';
	import { dateTime, errorMessage } from '#lib/format.ts';

	/** The DOM stays light; the server keeps more. */
	const SHOWN = 300;
	const tones: Record<LogLevel, ChipTone> = {
		error: 'bad',
		warn: 'wanted',
		info: 'ok',
		debug: 'off',
		trace: 'off'
	};

	let level = $state<LogLevel>('info');
	let lines = $state<LogLine[]>([]);
	let error = $state<string | null>(null);

	async function load() {
		try {
			lines = await api.logs(level);
			error = null;
		} catch (e) {
			error = errorMessage(e);
		}
	}

	$effect(() => {
		void level;
		load();
		const timer = setInterval(load, 5000);
		return () => clearInterval(timer);
	});

	const time = (iso: string) => new Date(iso).toLocaleTimeString();
</script>

<section aria-labelledby="logs-heading" class="flex flex-col gap-3">
	<div class="flex flex-wrap items-center justify-between gap-3">
		<h2 id="logs-heading" class="section-title text-lg">Logs</h2>
		<label class="flex items-center gap-2 text-sm text-ink-2">
			Show
			<select bind:value={level} class="field-sm w-auto font-sans">
				<option value="error">Errors</option>
				<option value="warn">Warnings and errors</option>
				<option value="info">Info and above</option>
				<option value="debug">Debug and above</option>
				<option value="trace">Everything</option>
			</select>
		</label>
	</div>
	<p class="m-0 text-sm text-muted">
		The latest lines since kaze started, newest first. Set <code>KAZE_LOG</code> for more detail; Docker
		or journald keep the full log.
	</p>
	{#if error}
		<p class="card m-0 border-bad-line p-4 text-bad">{error}</p>
	{:else if lines.length === 0}
		<p class="card m-0 p-4 text-sm text-muted">Nothing logged at this level yet.</p>
	{:else}
		<ol
			class="card m-0 flex max-h-[480px] list-none flex-col overflow-y-auto p-0 font-mono text-xs"
		>
			{#each lines.slice(0, SHOWN) as line, i (i)}
				<li
					class="flex flex-wrap items-baseline gap-x-2.5 gap-y-1 border-t border-raised px-3 py-1.5 first:border-t-0"
				>
					<span class="text-muted" title={dateTime(line.time)}>{time(line.time)}</span>
					<StateChip tone={tones[line.level]} label={line.level} />
					<span class="text-muted">{line.target.replace(/^kaze::/, '')}</span>
					<span class="min-w-0 flex-1 break-words whitespace-pre-wrap text-ink">{line.message}</span
					>
					{#if line.spans}<span class="w-full text-muted">{line.spans}</span>{/if}
				</li>
			{/each}
		</ol>
		{#if lines.length > SHOWN}
			<p class="m-0 text-xs text-muted">Showing the newest {SHOWN} of {lines.length}.</p>
		{/if}
	{/if}
</section>
