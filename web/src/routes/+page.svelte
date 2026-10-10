<script lang="ts">
	import { api, type SeriesSummary } from '#lib/api/index.ts';
	import Cover from '#lib/components/Cover.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import ProgressBar from '#lib/components/ProgressBar.svelte';
	import Segmented from '#lib/components/Segmented.svelte';
	import { errorMessage, num } from '#lib/format.ts';
	import { live } from '#lib/live.svelte.ts';

	type Show = 'all' | 'missing' | 'unmonitored';

	let series = $state<SeriesSummary[] | null>(null);
	let error = $state<string | null>(null);
	let filter = $state('');
	let show = $state<Show>('all');

	$effect(() => {
		void live.libraryVersion;
		api
			.series()
			.then((s) => {
				series = s;
				error = null;
			})
			.catch((e) => (error = errorMessage(e)));
	});

	const shown = $derived.by(() => {
		const q = filter.trim().toLowerCase();
		return (series ?? [])
			.filter((s) => !q || s.title.toLowerCase().includes(q))
			.filter((s) =>
				show === 'missing' ? s.wanted > 0 : show === 'unmonitored' ? s.monitor === 'none' : true
			)
			.sort((a, b) => a.title.localeCompare(b.title));
	});

	const totals = $derived(
		(series ?? []).reduce(
			(t, s) => ({ downloaded: t.downloaded + s.downloaded, wanted: t.wanted + s.wanted }),
			{ downloaded: 0, wanted: 0 }
		)
	);

	const statusLabel = (s: SeriesSummary) => {
		const status = s.status === 'unknown' ? '' : s.status[0].toUpperCase() + s.status.slice(1);
		return [status, s.monitor === 'none' ? 'unmonitored' : ''].filter(Boolean).join(' · ');
	};
</script>

<svelte:head><title>Library · kaze</title></svelte:head>

<header class="flex flex-wrap items-end justify-between gap-5">
	<div class="flex flex-col gap-1">
		<h1 class="page-title">Library</h1>
		{#if series}
			<p class="m-0 text-muted">
				{num(series.length)} series · {num(totals.downloaded)} chapters on disk · {num(
					totals.wanted
				)} wanted
			</p>
		{/if}
	</div>
	<div class="flex flex-wrap items-center gap-3">
		<label class="relative flex items-center">
			<span class="sr-only">Filter library</span>
			<Icon
				name="search"
				size={16}
				stroke={2}
				class="pointer-events-none absolute left-3 text-muted"
			/>
			<input
				type="search"
				placeholder="Filter by title"
				bind:value={filter}
				class="field w-60 pl-9"
			/>
		</label>
		<Segmented
			label="Show"
			bind:value={show}
			options={[
				{ value: 'all', label: 'All' },
				{ value: 'missing', label: 'Missing chapters' },
				{ value: 'unmonitored', label: 'Unmonitored' }
			]}
		/>
		<a href="/add" class="btn-primary"><Icon name="add" size={16} stroke={2.4} /> Add series</a>
	</div>
</header>

{#if error}
	<p class="card m-0 border-bad-line p-4 text-bad">Couldn't load the library: {error}</p>
{:else if series === null}
	<div
		class="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-x-[22px] gap-y-7 sm:grid-cols-[repeat(auto-fill,minmax(172px,1fr))]"
	>
		{#each { length: 8 }, i (i)}
			<div class="aspect-[2/3] animate-pulse rounded-[10px] bg-surface"></div>
		{/each}
	</div>
{:else if series.length === 0}
	<div class="card flex flex-col items-start gap-3 p-8">
		<h2 class="section-title">Nothing followed yet</h2>
		<p class="m-0 max-w-[60ch] text-ink-2">
			Search the sources for a series, or paste its link. kaze downloads the chapters you choose and
			keeps watching for new ones.
		</p>
		<a href="/add" class="btn-primary"
			><Icon name="add" size={16} stroke={2.4} /> Add your first series</a
		>
	</div>
{:else if shown.length === 0}
	<p class="m-0 text-muted">No series match.</p>
{:else}
	<section
		aria-label="Series"
		class="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-x-[22px] gap-y-7 sm:grid-cols-[repeat(auto-fill,minmax(172px,1fr))]"
	>
		{#each shown as s (s.id)}
			<a
				href="/series/{s.id}"
				class="group flex flex-col gap-2.5 text-ink no-underline hover:text-ink"
			>
				<div class="relative">
					<Cover
						id={s.id}
						title={s.title}
						class="rounded-[10px] transition-transform group-hover:-translate-y-0.5"
					/>
					<span
						class="absolute top-2.5 left-2.5 rounded-md bg-rail/75 px-2 py-0.5 text-[11px] font-medium text-ink-2 capitalize"
						>{s.source}</span
					>
					{#if s.wanted > 0}
						<span
							class="absolute top-2.5 right-2.5 rounded-md bg-wanted px-2 py-0.5 font-mono text-xs font-medium text-wanted-ink"
							title="{s.wanted} wanted">+{num(s.wanted)}</span
						>
					{/if}
				</div>
				<div class="flex flex-col gap-1.5">
					<span class="leading-tight font-semibold group-hover:text-seal-soft">{s.title}</span>
					<span class="flex justify-between gap-2 text-xs text-muted">
						<span class="truncate">{statusLabel(s)}</span>
						<span class="font-mono">{num(s.downloaded)} / {num(s.chapters)}</span>
					</span>
					<ProgressBar
						value={s.chapters ? s.downloaded / s.chapters : 0}
						height={3}
						color={s.wanted ? 'var(--color-wanted)' : 'var(--color-disk)'}
					/>
				</div>
			</a>
		{/each}
	</section>
{/if}
