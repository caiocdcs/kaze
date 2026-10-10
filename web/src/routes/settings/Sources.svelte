<script lang="ts">
	import { api, type SourceInfo } from '#lib/api/index.ts';
	import { errorMessage } from '#lib/format.ts';

	let sources = $state<SourceInfo[] | null>(null);
	let error = $state<string | null>(null);

	$effect(() => {
		api
			.sources()
			.then((s) => (sources = s))
			.catch((e) => (error = errorMessage(e)));
	});
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
				</li>
			{/each}
		</ul>
	{/if}
</div>
