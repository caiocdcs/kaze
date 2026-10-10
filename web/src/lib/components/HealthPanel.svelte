<script lang="ts">
	import { api, type HealthCheck } from '#lib/api/index.ts';
	import { errorMessage } from '#lib/format.ts';

	let { version = 0 }: { version?: number } = $props();

	let checks = $state<HealthCheck[] | null>(null);
	let error = $state<string | null>(null);

	$effect(() => {
		void version;
		api
			.health()
			.then((c) => {
				checks = c;
				error = null;
			})
			.catch((e) => (error = errorMessage(e)));
	});

	const look = {
		ok: ['OK', 'bg-ok-wash text-ok'],
		slow: ['Slow', 'bg-wanted-wash text-wanted'],
		down: ['Down', 'bg-bad-wash text-bad'],
		off: ['Off', 'bg-raised text-muted']
	} as const;
</script>

<aside aria-labelledby="health" class="card flex flex-col gap-3.5 p-5">
	<h2 id="health" class="m-0 text-[15px] font-semibold">Health</h2>
	{#if error}
		<p class="m-0 text-sm text-bad">{error}</p>
	{:else if checks === null}
		{#each { length: 4 }, i (i)}
			<div class="h-6 animate-pulse rounded bg-raised"></div>
		{/each}
	{:else}
		<ul class="m-0 flex list-none flex-col gap-2.5 p-0">
			{#each checks as c (c.kind + c.name)}
				{@const [label, tone] = look[c.state]}
				<li class="flex items-center gap-3 text-sm">
					<span class="w-[52px] flex-none rounded-md py-px text-center text-xs font-semibold {tone}"
						>{label}</span
					>
					<span class="min-w-0 flex-1 truncate">{c.name}</span>
					<span class="max-w-[50%] truncate text-xs text-muted" title={c.detail}>{c.detail}</span>
				</li>
			{/each}
		</ul>
	{/if}
</aside>
