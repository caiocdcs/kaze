<script lang="ts">
	import { untrack } from 'svelte';
	import { api, type MetadataHit, type SeriesDetail } from '#lib/api/index.ts';
	import Cover from '#lib/components/Cover.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import { errorMessage } from '#lib/format.ts';
	import { live } from '#lib/live.svelte.ts';
	import { toasts } from '#lib/toast.svelte.ts';

	let { series, onchange }: { series: SeriesDetail; onchange: (s: SeriesDetail) => void } =
		$props();

	let busy = $state(false);
	let searching = $state(false);
	let query = $state('');
	let hits = $state<MetadataHit[] | null>(null);
	let searchError = $state<string | null>(null);
	/** A link changed: reload the series once its refresh is done. */
	let pending = $state(false);

	$effect(() => {
		void live.jobsVersion;
		if (!untrack(() => pending)) return;
		const refreshing = live.active.some(
			(j) =>
				j.series_id === series.id &&
				(j.job.type === 'refresh_metadata' || j.job.type === 'refresh_series')
		);
		if (!refreshing) {
			pending = false;
			api.seriesDetail(series.id).then(onchange, toasts.error);
		}
	});

	const byHand = $derived(series.metadata_link === 'manual');
	const status = $derived.by(() => {
		if (series.anilist_id !== null) return byHand ? 'Linked by hand' : 'Linked automatically';
		if (byHand) return 'Unlinked by hand: metadata comes from its source';
		if (!series.metadata_checked_at) return 'Looking for it on AniList…';
		return 'Not found on AniList; kaze tries again weekly';
	});

	async function link(anilistId: number | null, done: string) {
		busy = true;
		try {
			onchange(await api.linkMetadata(series.id, anilistId));
			pending = true;
			toasts.show(done);
			searching = false;
		} catch (e) {
			toasts.error(e);
		} finally {
			busy = false;
		}
	}

	function startSearching() {
		searching = true;
		query = series.title;
		hits = null;
		searchError = null;
	}

	async function search(event: SubmitEvent) {
		event.preventDefault();
		hits = null;
		searchError = null;
		busy = true;
		try {
			hits = await api.searchMetadata(query.trim());
		} catch (e) {
			searchError = errorMessage(e);
		} finally {
			busy = false;
		}
	}

	function unlink() {
		if (!confirm(`Unlink ${series.title}? Its metadata and cover come from its source again.`))
			return;
		link(null, 'Unlinked from AniList');
	}
</script>

<aside aria-labelledby="metadata-heading" class="card flex flex-[1_1_300px] flex-col gap-3.5 p-5">
	<h2 id="metadata-heading" class="m-0 text-[15px] font-semibold">Metadata</h2>
	<p class="m-0 text-[13px] text-muted">
		Title, summary, genres and cover come from AniList once the series is linked.
	</p>
	<div class="flex items-center gap-2.5 rounded-[10px] bg-ground py-2 pr-2 pl-3">
		<span
			class="size-2 flex-none rounded-full {series.anilist_id !== null ? 'bg-disk' : 'bg-raised'}"
		></span>
		<span class="flex min-w-0 flex-1 flex-col">
			{#if series.anilist_id !== null}
				<a
					href="https://anilist.co/manga/{series.anilist_id}"
					target="_blank"
					rel="noreferrer"
					class="truncate text-sm text-ink">AniList #{series.anilist_id} ↗</a
				>
			{:else}
				<span class="text-sm text-ink">Not linked</span>
			{/if}
			<span class="text-xs text-muted">{status}</span>
		</span>
		{#if series.anilist_id !== null}
			<button
				type="button"
				class="icon-btn hover:text-bad"
				aria-label="Unlink from AniList"
				title="Unlink"
				disabled={busy}
				onclick={unlink}
			>
				<Icon name="trash" />
			</button>
		{/if}
	</div>

	{#if !searching}
		<button type="button" class="btn-ghost" onclick={startSearching}>
			<Icon name="search" size={16} stroke={2} />
			{series.anilist_id !== null ? 'Change' : 'Link'}
		</button>
	{:else}
		<form class="flex flex-col gap-2.5" onsubmit={search}>
			<input
				type="search"
				bind:value={query}
				class="field-sm"
				aria-label="Search AniList for the series"
			/>
			<div class="flex justify-end gap-2">
				<button type="button" class="btn-ghost" onclick={() => (searching = false)}>Cancel</button>
				<button type="submit" class="btn-primary" disabled={busy || !query.trim()}>
					<Icon name="search" size={16} stroke={2} /> Search
				</button>
			</div>
		</form>
		{#if searchError}
			<p class="m-0 text-sm text-bad">{searchError}</p>
		{:else if hits?.length === 0}
			<p class="m-0 text-sm text-muted">Nothing found.</p>
		{/if}
		{#each hits ?? [] as hit (hit.anilist_id)}
			<div
				class="flex items-center gap-3 rounded-xl border border-raised bg-surface-2 py-2 pr-2.5 pl-2"
			>
				<Cover src={hit.cover_url} title={hit.title} size="sm" class="w-9 flex-none rounded-md" />
				<span class="flex min-w-0 flex-1 flex-col">
					<a href={hit.url} target="_blank" rel="noreferrer" class="truncate text-sm text-ink"
						>{hit.title}</a
					>
					<span class="truncate text-xs text-muted">
						{[hit.romaji, hit.year, hit.one_shot ? 'One-shot' : null].filter(Boolean).join(' · ')}
					</span>
				</span>
				<button
					type="button"
					class="btn-ghost h-9"
					disabled={busy || hit.anilist_id === series.anilist_id}
					onclick={() => link(hit.anilist_id, `Linked to ${hit.title}; metadata updates shortly`)}
					>Use</button
				>
			</div>
		{/each}
	{/if}
</aside>
