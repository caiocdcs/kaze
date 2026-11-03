<script lang="ts">
	import { api, type SeriesDetail, type SourceInfo } from '#lib/api/index.ts';
	import type { SeriesHit } from '#lib/api/types/SeriesHit.ts';
	import Cover from '#lib/components/Cover.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import { errorMessage } from '#lib/format.ts';
	import { toasts } from '#lib/toast.svelte.ts';

	let { series, onchange }: { series: SeriesDetail; onchange: (s: SeriesDetail) => void } =
		$props();

	let all = $state<SourceInfo[]>([]);
	let busy = $state(false);
	let adding = $state(false);
	let source = $state('');
	let query = $state('');
	let hits = $state<SeriesHit[] | null>(null);
	let searchError = $state<string | null>(null);

	$effect(() => {
		api.sources().then((s) => (all = s), toasts.error);
	});

	const name = (id: string) => all.find((s) => s.id === id)?.name ?? id;
	const linked = $derived(new Set(series.sources.map((s) => s.source)));
	/** Sources that offer the series' language and aren't used yet. */
	const addable = $derived(
		all.filter((s) => !linked.has(s.id) && s.languages.includes(series.language))
	);

	async function act(work: () => Promise<SeriesDetail>, done?: string) {
		busy = true;
		try {
			onchange(await work());
			if (done) toasts.show(done);
		} catch (e) {
			toasts.error(e);
		} finally {
			busy = false;
		}
	}

	function move(index: number, by: -1 | 1) {
		const order = series.sources.map((s) => s.source);
		[order[index], order[index + by]] = [order[index + by], order[index]];
		act(() => api.reorderSources(series.id, order));
	}

	function remove(id: string) {
		if (!confirm(`Stop downloading ${series.title} from ${name(id)}? Files stay on disk.`)) return;
		act(() => api.removeSource(series.id, id), `Removed ${name(id)}`);
	}

	function startAdding() {
		adding = true;
		source = addable[0]?.id ?? '';
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
			const [result] = await api.search(query.trim(), source, series.language);
			if (result?.error) searchError = result.error;
			else hits = result?.hits ?? [];
		} catch (e) {
			searchError = errorMessage(e);
		} finally {
			busy = false;
		}
	}

	async function add(hit: SeriesHit) {
		await act(() => api.addSource(series.id, source, hit.key), `Added ${name(source)}`);
		adding = false;
	}
</script>

<aside aria-labelledby="sources-heading" class="card flex flex-[1_1_300px] flex-col gap-3.5 p-5">
	<h2 id="sources-heading" class="m-0 text-[15px] font-semibold">Sources</h2>
	<p class="m-0 text-[13px] text-muted">
		Tried in this order. Later sources fill missing chapters and take over when one breaks. The
		first gives the metadata unless the series is linked to AniList.
	</p>
	<ol class="m-0 flex list-none flex-col gap-2 p-0">
		{#each series.sources as s, i (s.source)}
			<li class="flex items-center gap-2.5 rounded-[10px] bg-ground py-2 pr-2 pl-3">
				<span
					class="size-2 flex-none rounded-full {s.failing ? 'bg-bad' : 'bg-disk'}"
					title={s.failing ? 'Failing' : 'Working'}
				></span>
				<span class="flex min-w-0 flex-1 flex-col">
					<a href={s.url} target="_blank" rel="noreferrer" class="truncate text-sm text-ink"
						>{name(s.source)} ↗</a
					>
					<span class="text-xs text-muted">
						{i === 0 ? 'Primary' : `Fallback ${i}`}{s.failing ? ' · failing' : ''}
					</span>
				</span>
				<button
					type="button"
					class="icon-btn"
					aria-label="Try {name(s.source)} earlier"
					title="Move up"
					disabled={busy || i === 0}
					onclick={() => move(i, -1)}>↑</button
				>
				<button
					type="button"
					class="icon-btn"
					aria-label="Try {name(s.source)} later"
					title="Move down"
					disabled={busy || i === series.sources.length - 1}
					onclick={() => move(i, 1)}>↓</button
				>
				<button
					type="button"
					class="icon-btn hover:text-bad"
					aria-label="Remove {name(s.source)}"
					title="Remove"
					disabled={busy || series.sources.length === 1}
					onclick={() => remove(s.source)}
				>
					<Icon name="trash" />
				</button>
			</li>
		{/each}
	</ol>

	{#if !adding}
		<button
			type="button"
			class="btn-ghost"
			disabled={addable.length === 0}
			title={addable.length === 0 ? 'No other source offers this language' : undefined}
			onclick={startAdding}
		>
			<Icon name="add" size={16} stroke={2} /> Add source
		</button>
	{:else}
		<form class="flex flex-col gap-2.5" onsubmit={search}>
			<div class="flex gap-2">
				<select bind:value={source} class="field-sm w-auto font-sans" aria-label="Source">
					{#each addable as s (s.id)}
						<option value={s.id}>{s.name}</option>
					{/each}
				</select>
				<input
					type="search"
					bind:value={query}
					class="field-sm min-w-0 flex-1"
					aria-label="Search for the series"
				/>
			</div>
			<div class="flex justify-end gap-2">
				<button type="button" class="btn-ghost" onclick={() => (adding = false)}>Cancel</button>
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
		{#each hits ?? [] as hit (hit.key)}
			<div
				class="flex items-center gap-3 rounded-xl border border-raised bg-surface-2 py-2 pr-2.5 pl-2"
			>
				<Cover src={hit.cover_url} title={hit.title} size="sm" class="w-9 flex-none rounded-md" />
				<a
					href={hit.url}
					target="_blank"
					rel="noreferrer"
					class="min-w-0 flex-1 truncate text-sm text-ink">{hit.title}</a
				>
				<button type="button" class="btn-ghost h-9" disabled={busy} onclick={() => add(hit)}
					>Use</button
				>
			</div>
		{/each}
	{/if}
</aside>
