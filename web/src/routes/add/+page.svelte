<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { onMount } from 'svelte';
	import {
		api,
		type Lookup,
		type SeriesSummary,
		type SourceInfo,
		type SourceResults
	} from '#lib/api/index.ts';
	import type { Monitor } from '#lib/api/types/Monitor.ts';
	import type { SeriesHit } from '#lib/api/types/SeriesHit.ts';
	import Cover from '#lib/components/Cover.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import MonitorPicker from '#lib/components/MonitorPicker.svelte';
	import { errorMessage, languageName, offeredLanguages } from '#lib/format.ts';
	import { toasts } from '#lib/toast.svelte.ts';

	type Group = {
		source: string;
		state: 'loading' | 'done' | 'failed';
		hits: SeriesHit[];
		error?: string;
	};
	type Pick = { source: string; hit: SeriesHit };

	let query = $state(page.url.searchParams.get('q') ?? '');
	let sources = $state<SourceInfo[]>([]);
	let language = $state('en');
	let enabled = $state<Set<string>>(new Set());
	let groups = $state<Group[]>([]);
	let followed = $state<SeriesSummary[]>([]);
	let lookupError = $state<string | null>(null);
	let searching = $state(false);

	let pick = $state<Pick | null>(null);
	let preview = $state<Lookup | null>(null);
	let monitor = $state<Monitor>('future');
	let from = $state('');
	let following = $state(false);

	// onMount, not $effect: reading `query` here must not re-run the search on every keystroke.
	onMount(() => {
		api.sources().then((s) => {
			sources = s;
			enabled = new Set(s.map((x) => x.id));
		});
		api.series().then((s) => (followed = s));
		// A search from the URL waits for the default language.
		api
			.settings()
			.then((v) => (language = v.settings.default_language))
			.catch(() => {})
			.finally(() => {
				if (query) search();
			});
	});

	const languages = $derived(offeredLanguages(sources));
	// Only sources that offer the chosen language are searched.
	const offering = $derived(sources.filter((s) => s.languages.includes(language)));
	const sourceName = (id: string) => sources.find((s) => s.id === id)?.name ?? id;
	const followedId = (hit: SeriesHit) =>
		followed.find((s) => s.url === hit.url && s.language === language)?.id;
	const isUrl = (q: string) => /^https?:\/\//i.test(q.trim());

	async function search(event?: SubmitEvent) {
		event?.preventDefault();
		const q = query.trim();
		if (!q) return;
		pick = null;
		preview = null;
		lookupError = null;
		history.replaceState(history.state, '', `?q=${encodeURIComponent(q)}`);

		if (isUrl(q)) {
			groups = [];
			searching = true;
			try {
				const found = await api.lookup(q, language);
				if (found.followed_id !== null) {
					goto(`/series/${found.followed_id}`);
					return;
				}
				preview = found;
				pick = {
					source: found.source,
					hit: {
						key: found.key,
						title: found.series.title,
						url: found.series.url,
						cover_url: found.series.cover_url
					}
				};
			} catch (e) {
				lookupError = errorMessage(e);
			} finally {
				searching = false;
			}
			return;
		}

		// One request per source, so a slow site doesn't hold back the others.
		const ids = offering.map((s) => s.id).filter((id) => enabled.has(id));
		searching = true;
		if (ids.length === 0) {
			const all = await api.search(q, undefined, language).catch((e) => {
				lookupError = errorMessage(e);
				return [] as SourceResults[];
			});
			groups = all.map(toGroup);
			searching = false;
			return;
		}
		groups = ids.map((source) => ({ source, state: 'loading', hits: [] }));
		await Promise.all(ids.map((id) => searchOne(q, id)));
		searching = false;
	}

	async function searchOne(q: string, source: string) {
		const set = (g: Group) => (groups = groups.map((x) => (x.source === source ? g : x)));
		set({ source, state: 'loading', hits: [] });
		try {
			const [result] = await api.search(q, source, language);
			set(toGroup(result ?? { source, hits: [] }));
		} catch (e) {
			set({ source, state: 'failed', hits: [], error: errorMessage(e) });
		}
	}

	function toGroup(r: SourceResults): Group {
		return r.error
			? { source: r.source, state: 'failed', hits: [], error: r.error }
			: { source: r.source, state: 'done', hits: r.hits ?? [] };
	}

	async function choose(source: string, hit: SeriesHit) {
		pick = { source, hit };
		preview = null;
		try {
			const found = await api.lookup(hit.url, language);
			if (pick?.hit.url === hit.url) preview = found;
		} catch {
			// The preview is a nicety; following still works without it.
		}
	}

	function toggleSource(id: string) {
		const next = new Set(enabled);
		if (next.has(id)) next.delete(id);
		else next.add(id);
		enabled = next;
	}

	async function follow() {
		if (!pick) return;
		const f = from.trim();
		if (f && !/^\d+(\.\d+)?$/.test(f)) {
			toasts.show('“Only from chapter” must be a number like 12 or 12.5', 'error');
			return;
		}
		following = true;
		try {
			const { id } = await api.follow({
				source: pick.source,
				key: pick.hit.key,
				// A pasted link may be read in the source's own language.
				language: preview?.language ?? language,
				monitor,
				monitor_from: f || null
			});
			toasts.show(`Following ${pick.hit.title}`);
			goto(`/series/${id}`);
		} catch (e) {
			toasts.error(e);
		} finally {
			following = false;
		}
	}
</script>

<svelte:head><title>Add series · kaze</title></svelte:head>

<div class="flex flex-col gap-1">
	<h1 class="page-title">Add series</h1>
	<p class="m-0 text-muted">Search every source at once, or paste a series link.</p>
</div>

<form class="flex flex-col gap-3" onsubmit={search}>
	<div class="flex flex-wrap gap-2.5">
		<label class="relative flex flex-[1_1_360px] items-center">
			<span class="sr-only">Title or URL</span>
			<Icon
				name="search"
				size={20}
				stroke={2}
				class="pointer-events-none absolute left-4 text-muted"
			/>
			<!-- svelte-ignore a11y_autofocus -->
			<input
				type="search"
				bind:value={query}
				autofocus
				placeholder="Title, or https://weebcentral.com/series/…"
				class="h-[52px] w-full rounded-xl border border-line-input bg-surface pr-4 pl-[46px] text-[17px] text-ink placeholder:text-muted"
			/>
		</label>
		<button
			type="submit"
			class="btn-primary h-[52px] rounded-xl px-6 text-base"
			disabled={searching}
		>
			{isUrl(query) ? 'Look up' : 'Search'}
		</button>
	</div>
	{#if languages.length > 1}
		<label class="flex items-center gap-2 text-[13px] text-muted">
			Language
			<select
				bind:value={language}
				onchange={() => groups.length && search()}
				class="field-sm w-auto font-sans text-ink"
			>
				{#each languages as l (l)}
					<option value={l}>{languageName(l)}</option>
				{/each}
			</select>
		</label>
	{/if}
	{#if offering.length > 1 && !isUrl(query)}
		<fieldset class="m-0 flex flex-wrap items-center gap-2 border-0 p-0">
			<legend class="float-left mr-1.5 text-[13px] text-muted">Sources</legend>
			{#each offering as s (s.id)}
				<label
					class="inline-flex h-9 cursor-pointer items-center gap-2 rounded-full px-3 text-sm {enabled.has(
						s.id
					)
						? 'bg-raised text-ink'
						: 'border border-line-strong text-muted'}"
				>
					<input type="checkbox" checked={enabled.has(s.id)} onchange={() => toggleSource(s.id)} />
					{s.name}
				</label>
			{/each}
		</fieldset>
	{/if}
</form>

{#if lookupError}
	<p class="card m-0 border-bad-line p-4 text-bad">{lookupError}</p>
{/if}

<div class="flex flex-wrap items-start gap-7">
	<div class="flex min-w-0 flex-[999_1_520px] flex-col gap-6">
		{#each groups as g (g.source)}
			<section aria-label="{sourceName(g.source)} results" class="flex flex-col gap-2.5">
				<h2 class="m-0 flex items-center gap-2.5 text-[15px] font-semibold">
					{sourceName(g.source)}
					{#if g.state === 'loading'}
						<span class="rounded-full bg-raised px-2 py-px text-xs font-medium text-muted"
							>Searching…</span
						>
					{:else if g.state === 'failed'}
						<span class="rounded-full bg-bad-wash px-2 py-px text-xs font-medium text-bad"
							>Failed</span
						>
					{:else}
						<span class="rounded-full bg-raised px-2 py-px text-xs font-medium text-muted"
							>{g.hits.length} result{g.hits.length === 1 ? '' : 's'}</span
						>
					{/if}
				</h2>
				{#if g.state === 'failed'}
					<div
						class="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-dashed border-bad-line px-4 py-3.5 text-sm text-bad"
					>
						<span>{g.error}. Other sources are unaffected.</span>
						<button
							type="button"
							class="btn-danger h-10"
							onclick={() => searchOne(query.trim(), g.source)}>Try again</button
						>
					</div>
				{:else if g.state === 'loading'}
					{#each { length: 2 }, i (i)}
						<div class="h-[86px] animate-pulse rounded-xl bg-surface-2"></div>
					{/each}
				{:else if g.hits.length === 0}
					<p class="m-0 text-sm text-muted">Nothing found.</p>
				{/if}
				{#each g.hits as hit (hit.key)}
					{@const inLibrary = followedId(hit)}
					{@const selected = pick?.hit.url === hit.url}
					<div
						class="flex items-center gap-3.5 rounded-xl border py-2.5 pr-3.5 pl-2.5 {selected
							? 'border-seal-line bg-seal-wash'
							: 'border-raised bg-surface-2'}"
					>
						<Cover
							src={hit.cover_url}
							title={hit.title}
							size="sm"
							class="w-11 flex-none rounded-md"
						/>
						<div class="flex min-w-0 flex-1 flex-col gap-0.5">
							<span class="font-semibold">{hit.title}</span>
							<a
								href={hit.url}
								target="_blank"
								rel="noreferrer"
								class="truncate text-[13px] text-muted">{hit.url.replace(/^https?:\/\//, '')}</a
							>
						</div>
						{#if inLibrary !== undefined}
							<a href="/series/{inLibrary}" class="text-sm whitespace-nowrap text-disk"
								>In library →</a
							>
						{:else}
							<button
								type="button"
								class="{selected ? 'btn-primary' : 'btn-ghost'} h-10"
								aria-pressed={selected}
								onclick={() => choose(g.source, hit)}>{selected ? 'Selected' : 'Follow'}</button
							>
						{/if}
					</div>
				{/each}
			</section>
		{/each}
	</div>

	{#if pick}
		<aside
			aria-label="Follow"
			class="flex flex-[1_1_340px] flex-col gap-[18px] rounded-2xl border border-seal-line bg-surface p-[22px] md:sticky md:top-8"
		>
			<div class="flex items-center gap-3.5">
				<Cover
					src={pick.hit.cover_url}
					title={pick.hit.title}
					size="xs"
					class="w-14 flex-none rounded-lg"
				/>
				<div class="min-w-0">
					<p class="m-0 text-[13px] text-muted">
						Follow from {sourceName(pick.source)} · {languageName(preview?.language ?? language)}
					</p>
					<h2 class="m-0 font-display text-[22px] leading-tight font-bold">{pick.hit.title}</h2>
				</div>
			</div>
			{#if preview}
				<p class="m-0 flex flex-wrap gap-x-3 text-[13px] text-ink-2">
					{#if preview.series.authors.length}<span>{preview.series.authors.join(', ')}</span>{/if}
					{#if preview.series.year}<span>{preview.series.year}</span>{/if}
					{#if preview.series.status !== 'unknown'}<span class="capitalize"
							>{preview.series.status}</span
						>{/if}
				</p>
				{#if preview.series.description}
					<p class="m-0 line-clamp-4 text-sm text-muted">{preview.series.description}</p>
				{/if}
			{/if}
			<MonitorPicker
				name="follow-monitor"
				legend="Download"
				bind:value={monitor}
				hints={{
					all: 'Every chapter now, new ones as they come',
					future: 'Nothing now; everything released from today',
					none: 'Track it; pick chapters later'
				}}
			/>
			<label class="flex items-center justify-between gap-3 text-sm text-ink-2">
				Only from chapter
				<input
					type="text"
					inputmode="decimal"
					placeholder="any"
					bind:value={from}
					class="field-sm w-[90px]"
				/>
			</label>
			<div class="flex flex-col gap-1 rounded-[10px] bg-ground px-3.5 py-3">
				<span class="text-xs text-muted">Files go to</span>
				<span class="font-mono text-[13px] break-all">library/{pick.hit.title}/</span>
			</div>
			<button
				type="button"
				class="btn-primary h-12 rounded-xl text-base"
				disabled={following}
				onclick={follow}>{following ? 'Following…' : 'Follow series'}</button
			>
		</aside>
	{/if}
</div>
