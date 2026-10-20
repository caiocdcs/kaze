<script lang="ts">
	import { api, type Candidate, type Match, type SourceInfo } from '#lib/api/index.ts';
	import type { Monitor } from '#lib/api/types/Monitor.ts';
	import { compareNumbers } from '#lib/chapters.ts';
	import Cover from '#lib/components/Cover.svelte';
	import { errorMessage, languageName, num } from '#lib/format.ts';
	import { toasts } from '#lib/toast.svelte.ts';

	type Row = {
		candidate: Candidate;
		matches?: Match[];
		searching?: boolean;
		error?: string;
		pick?: Match;
		/** Defaults to the folder's own language, else the source's main one. */
		language?: string;
		monitor: Monitor;
		importing?: boolean;
		done?: { id: number; matched: number };
	};

	let rows = $state<Row[] | null>(null);
	let sources = $state<SourceInfo[]>([]);
	let error = $state<string | null>(null);

	$effect(() => {
		api
			.sources()
			.then((s) => (sources = s))
			.catch(() => {});
		api
			.importCandidates()
			.then((c) => (rows = c.map((candidate) => ({ candidate, monitor: 'all' }))))
			.catch((e) => (error = errorMessage(e)));
	});

	const languagesOf = (source: string) => sources.find((s) => s.id === source)?.languages ?? [];

	function choose(row: Row, m: Match) {
		row.pick = m;
		const offered = languagesOf(m.source);
		const own = row.candidate.language;
		row.language = own && offered.includes(own) ? own : offered[0];
	}

	function range(chapters: string[]): string {
		if (chapters.length === 0) return 'no chapter numbers found';
		const sorted = [...chapters].sort(compareNumbers);
		return sorted.length === 1
			? `chapter ${sorted[0]}`
			: `chapters ${sorted[0]}–${sorted[sorted.length - 1]}`;
	}

	async function find(row: Row) {
		row.searching = true;
		row.error = undefined;
		try {
			row.matches = await api.importMatches(row.candidate.folder);
			// A near-exact title is almost certainly right; preselect it.
			if (row.matches[0]?.score >= 0.95) choose(row, row.matches[0]);
		} catch (e) {
			row.error = errorMessage(e);
		} finally {
			row.searching = false;
		}
	}

	async function importRow(row: Row) {
		if (!row.pick) return;
		row.importing = true;
		try {
			row.done = await api.importFolder({
				folder: row.candidate.folder,
				source: row.pick.source,
				key: row.pick.hit.key,
				language: row.language,
				monitor: row.monitor,
				monitor_from: null
			});
			toasts.show(`Imported ${row.candidate.folder}: ${row.done.matched} chapters on disk`);
		} catch (e) {
			toasts.error(e);
		} finally {
			row.importing = false;
		}
	}

	const pending = $derived((rows ?? []).filter((r) => !r.done).length);
</script>

<svelte:head><title>Import · kaze</title></svelte:head>

<div class="flex flex-col gap-1">
	<h1 class="page-title">Import</h1>
	<p class="m-0 max-w-[70ch] text-muted">
		Folders in your library that kaze doesn't track yet, from Tranga or anything else. Match each to
		a series on a source; chapters already on disk are kept and marked as downloaded.
	</p>
</div>

{#if error}
	<p class="card m-0 border-bad-line p-4 text-bad">Couldn't read the library: {error}</p>
{:else if rows === null}
	<div class="h-48 animate-pulse rounded-[14px] bg-surface"></div>
{:else if rows.length === 0}
	<div class="card flex flex-col gap-2 p-8">
		<h2 class="section-title">Nothing to import</h2>
		<p class="m-0 text-ink-2">
			Every folder with .cbz files in the library already belongs to a followed series.
		</p>
	</div>
{:else}
	<p class="m-0 text-sm text-muted">{num(pending)} folder{pending === 1 ? '' : 's'} to import</p>
	<ul class="m-0 flex list-none flex-col gap-3 p-0">
		{#each rows as row ((row.candidate.language ?? '') + '/' + row.candidate.folder)}
			<li class="card flex flex-col gap-4 p-[18px] {row.done ? 'opacity-60' : ''}">
				<div class="flex flex-wrap items-center gap-3">
					<div class="min-w-0 flex-1">
						<h2 class="m-0 truncate text-base font-semibold">{row.candidate.folder}</h2>
						<p class="m-0 text-[13px] text-muted">
							{num(row.candidate.files)} files · {range(row.candidate.chapters)}
							{#if row.candidate.unparsed}· {row.candidate.unparsed} without a chapter number{/if}
							{#if row.candidate.language}· {languageName(row.candidate.language)} folder{/if}
						</p>
					</div>
					{#if row.done}
						<a href="/series/{row.done.id}" class="text-sm text-disk">Imported · open series →</a>
					{:else if !row.matches}
						<button
							type="button"
							class="btn-ghost"
							disabled={row.searching}
							onclick={() => find(row)}
						>
							{row.searching ? 'Searching…' : 'Find match'}
						</button>
					{/if}
				</div>

				{#if row.error}
					<p class="m-0 text-sm text-bad">{row.error}</p>
				{/if}

				{#if row.matches && !row.done}
					{#if row.matches.length === 0}
						<p class="m-0 text-sm text-muted">
							No source found this title. Try Add series with a link, then come back.
						</p>
					{:else}
						<fieldset class="m-0 flex flex-col gap-2 border-0 p-0">
							<legend class="sr-only">Which series is {row.candidate.folder}?</legend>
							{#each row.matches as m (m.source + m.hit.key)}
								<label
									class="flex cursor-pointer items-center gap-3 rounded-xl border px-3 py-2 {row.pick ===
									m
										? 'border-seal-line bg-seal-wash'
										: 'border-raised bg-surface-2 hover:bg-raised'}"
								>
									<input
										type="radio"
										name="match-{row.candidate.folder}"
										checked={row.pick === m}
										onchange={() => choose(row, m)}
									/>
									<Cover
										src={m.hit.cover_url}
										title={m.hit.title}
										size="xs"
										class="w-8 flex-none rounded"
									/>
									<span class="min-w-0 flex-1">
										<span class="block truncate font-medium">{m.hit.title}</span>
										<span class="text-xs text-muted capitalize">{m.source}</span>
									</span>
									<span
										class="font-mono text-xs {m.score >= 0.95
											? 'text-ok'
											: m.score >= 0.85
												? 'text-wanted'
												: 'text-muted'}"
										title="Title similarity">{Math.round(m.score * 100)}%</span
									>
								</label>
							{/each}
						</fieldset>
						<div class="flex flex-wrap items-center gap-3">
							<label class="flex items-center gap-2 text-sm text-ink-2">
								Missing chapters
								<select bind:value={row.monitor} class="field-sm w-auto font-sans">
									<option value="all">Download them</option>
									<option value="future">Only new ones from now</option>
									<option value="none">Don't download</option>
								</select>
							</label>
							{#if row.pick && languagesOf(row.pick.source).length > 1}
								<label class="flex items-center gap-2 text-sm text-ink-2">
									Language
									<select bind:value={row.language} class="field-sm w-auto font-sans">
										{#each languagesOf(row.pick.source) as l (l)}
											<option value={l}>{languageName(l)}</option>
										{/each}
									</select>
								</label>
							{/if}
							<span class="flex-1"></span>
							<button
								type="button"
								class="btn-primary"
								disabled={!row.pick || row.importing}
								onclick={() => importRow(row)}>{row.importing ? 'Importing…' : 'Import'}</button
							>
						</div>
					{/if}
				{/if}
			</li>
		{/each}
	</ul>
{/if}
