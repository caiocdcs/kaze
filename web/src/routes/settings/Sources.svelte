<script lang="ts">
	import { api, type SourceInfo } from '#lib/api/index.ts';
	import StateChip, { type ChipTone } from '#lib/components/StateChip.svelte';
	import { ago, dateTime, errorMessage } from '#lib/format.ts';
	import { live } from '#lib/live.svelte.ts';

	let sources = $state<SourceInfo[] | null>(null);
	let error = $state<string | null>(null);

	$effect(() => {
		void live.jobsVersion;
		api
			.sources()
			.then((s) => (sources = s))
			.catch((e) => (error = errorMessage(e)));
	});

	function status(s: SourceInfo): { tone: ChipTone; label: string } {
		const h = s.health;
		if (s.failing) return { tone: 'bad', label: `Failing · ${h.failures_in_row} in a row` };
		if (h.failures_in_row > 0) return { tone: 'wanted', label: 'Recent errors' };
		if (h.last_success_at) return { tone: 'ok', label: 'Working' };
		return { tone: 'off', label: 'No jobs yet' };
	}

	// The error is worth showing until a later success replaces it.
	const showError = (s: SourceInfo) =>
		s.health.last_error !== null &&
		(s.health.failures_in_row > 0 || s.health.last_success_at === null);
</script>

<div class="flex max-w-3xl flex-col gap-4 pt-6">
	<p class="m-0 text-ink-2">
		Sites kaze can download from. Each is compiled in as a cargo feature; build without one to leave
		it out.
	</p>
	{#if error}
		<p class="card m-0 border-bad-line p-4 text-bad">{error}</p>
	{:else if sources}
		<ul class="m-0 flex list-none flex-col gap-2.5 p-0">
			{#each sources as s (s.id)}
				<li class="card flex flex-wrap items-center gap-4 p-4">
					<span
						class="flex size-10 flex-none items-center justify-center rounded-[10px] bg-raised font-display text-lg font-bold text-paper"
						aria-hidden="true">{s.name[0]}</span
					>
					<div class="min-w-0 flex-1">
						<h2 class="m-0 text-[15px] font-semibold">{s.name}</h2>
						<a href={s.base_url} target="_blank" rel="noreferrer" class="text-sm text-muted"
							>{s.base_url}</a
						>
					</div>
					<span class="font-mono text-xs text-muted uppercase"
						>{s.languages.join(' · ') || 'any'}</span
					>
					<div class="flex w-full flex-col gap-1.5 border-t border-line pt-3">
						<p class="m-0 flex flex-wrap items-center gap-x-4 gap-y-1 text-sm text-muted">
							<StateChip {...status(s)} />
							<span title={dateTime(s.health.last_success_at)}
								>Last success {ago(s.health.last_success_at)}</span
							>
							{#if s.health.last_error_at}
								<span title={dateTime(s.health.last_error_at)}
									>Last error {ago(s.health.last_error_at)}</span
								>
							{/if}
						</p>
						{#if showError(s)}
							<p class="m-0 font-mono text-xs break-all {s.failing ? 'text-bad' : 'text-muted'}">
								{s.health.last_error}
							</p>
						{/if}
					</div>
				</li>
			{/each}
		</ul>
	{/if}
</div>
