<script lang="ts">
	import { api, type Upcoming } from '#lib/api/index.ts';
	import Cover from '#lib/components/Cover.svelte';
	import { ago, errorMessage, num } from '#lib/format.ts';
	import { live } from '#lib/live.svelte.ts';

	let list = $state<Upcoming[] | null>(null);
	let error = $state<string | null>(null);

	$effect(() => {
		void live.libraryVersion;
		api.upcoming().then(
			(l) => {
				list = l;
				error = null;
			},
			(e) => (error = errorMessage(e))
		);
	});

	const groups = $derived(
		[
			{ state: 'expected', title: 'Expected this week' },
			{ state: 'later', title: 'Later' },
			{ state: 'stalled', title: 'Stalled' }
		].map((g) => ({ ...g, rows: (list ?? []).filter((u) => u.state === g.state) }))
	);

	function cadence(days: number | null): string {
		if (days === null) return 'One release so far';
		if (days === 1) return 'Daily';
		if (days % 7 === 0) return days === 7 ? 'Weekly' : `Every ${days / 7} weeks`;
		return `Every ${days} days`;
	}
</script>

<svelte:head><title>Upcoming · kaze</title></svelte:head>

<header class="flex flex-col gap-1">
	<h1 class="page-title">Upcoming</h1>
	<p class="m-0 text-muted">
		When each monitored series should get its next chapter, guessed from how often it got one
		lately.
	</p>
</header>

{#if error}
	<p class="card m-0 border-bad-line p-4 text-bad">Couldn't load: {error}</p>
{:else if list === null}
	<div class="h-72 animate-pulse rounded-[14px] bg-surface"></div>
{:else if list.length === 0}
	<p class="card m-0 p-5 text-muted">
		No release dates yet. They come with each source's chapters.
	</p>
{:else}
	{#each groups as g (g.state)}
		{#if g.rows.length}
			<section aria-labelledby="{g.state}-heading" class="flex flex-col gap-2.5">
				<h2 id="{g.state}-heading" class="section-title">
					{g.title} <span class="text-muted">({num(g.rows.length)})</span>
				</h2>
				<ul class="m-0 flex list-none flex-col gap-2 p-0">
					{#each g.rows as u (u.series_id)}
						<li>
							<a
								href="/series/{u.series_id}"
								class="flex items-center gap-3.5 rounded-xl border border-raised bg-surface-2 py-2 pr-4 pl-2 text-ink no-underline hover:border-line-strong"
							>
								<Cover
									id={u.series_id}
									title={u.title}
									size="sm"
									class="w-10 flex-none rounded-md"
								/>
								<span class="flex min-w-0 flex-1 flex-col">
									<span class="truncate font-semibold">{u.title}</span>
									<span class="text-[13px] text-muted">
										{cadence(u.cadence_days)} · last chapter {ago(u.last_published_at)}
									</span>
								</span>
								{#if u.state !== 'stalled' && u.expected_at}
									<span
										class="text-sm whitespace-nowrap {u.state === 'expected'
											? 'text-seal-soft'
											: 'text-ink-2'}"
										title={new Date(u.expected_at).toLocaleDateString()}
									>
										{new Date(u.expected_at) < new Date() ? 'Due' : 'Expected'}
										{ago(u.expected_at)}
									</span>
								{/if}
							</a>
						</li>
					{/each}
				</ul>
			</section>
		{/if}
	{/each}
{/if}
