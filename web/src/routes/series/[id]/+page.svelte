<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, type ChapterRecord, type SeriesDetail } from '#lib/api/index.ts';
	import type { Monitor } from '#lib/api/types/Monitor.ts';
	import { chapterName, compareNumbers, newestFirst } from '#lib/chapters.ts';
	import Cover from '#lib/components/Cover.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import MonitorPicker from '#lib/components/MonitorPicker.svelte';
	import Segmented from '#lib/components/Segmented.svelte';
	import StateChip, { type ChipTone } from '#lib/components/StateChip.svelte';
	import { ago, errorMessage, num } from '#lib/format.ts';
	import { live } from '#lib/live.svelte.ts';
	import { toasts } from '#lib/toast.svelte.ts';

	const PAGE = 50;

	const id = $derived(Number(page.params.id));
	let series = $state<SeriesDetail | null>(null);
	let error = $state<string | null>(null);
	let show = $state<'all' | 'wanted' | 'disk'>('all');
	let limit = $state(PAGE);
	let selected = $state<Set<number>>(new Set());
	let busy = $state(false);

	// Monitoring form, filled from the series and saved on change.
	let monitor = $state<Monitor>('all');
	let from = $state('');
	let refresh = $state<'default' | 'never' | 'custom'>('default');
	let refreshHours = $state(24);

	async function load() {
		try {
			const s = await api.seriesDetail(id);
			series = s;
			error = null;
			monitor = s.monitor;
			from = s.monitor_from ?? '';
			refresh = s.refresh_hours === null ? 'default' : s.refresh_hours === 0 ? 'never' : 'custom';
			if (s.refresh_hours) refreshHours = s.refresh_hours;
		} catch (e) {
			error = errorMessage(e);
		}
	}

	$effect(() => {
		void id;
		void live.libraryVersion;
		load();
	});

	const chapters = $derived([...(series?.chapters ?? [])].sort(newestFirst));
	const stats = $derived({
		total: chapters.length,
		disk: chapters.filter((c) => c.file_path).length,
		wanted: chapters.filter((c) => c.monitored && !c.file_path).length
	});
	const filtered = $derived(
		chapters.filter((c) =>
			show === 'wanted' ? c.monitored && !c.file_path : show === 'disk' ? !!c.file_path : true
		)
	);
	const visible = $derived(filtered.slice(0, limit));

	/** Jobs for this series' chapters, by chapter id. */
	const jobs = $derived(
		new Map(
			live.active
				.filter((j) => j.job.type === 'download_chapter' && j.series_id === id)
				.map((j) => [(j.job as { chapter_id: number }).chapter_id, j])
		)
	);

	function chapterState(c: ChapterRecord): { tone: ChipTone; label: string } {
		const job = jobs.get(c.id);
		if (job?.state === 'running') {
			const p = live.progress.get(job.id);
			return { tone: 'seal', label: p ? `Downloading ${p.done}/${p.total}` : 'Downloading' };
		}
		if (job) return { tone: 'seal', label: job.attempts > 0 ? 'Retrying' : 'Queued' };
		if (c.file_path) return { tone: 'disk', label: 'On disk' };
		if (c.monitored) return { tone: 'wanted', label: 'Wanted' };
		return { tone: 'off', label: 'Not monitored' };
	}

	async function act(work: () => Promise<unknown>, done?: string) {
		busy = true;
		try {
			await work();
			if (done) toasts.show(done);
			await load();
		} catch (e) {
			toasts.error(e);
		} finally {
			busy = false;
		}
	}

	function saveMonitoring() {
		const trimmed = from.trim();
		if (trimmed && !/^\d+(\.\d+)?$/.test(trimmed)) {
			toasts.show('“Starting at chapter” must be a number like 12 or 12.5', 'error');
			return;
		}
		act(() =>
			api.updateSeries(id, {
				monitor,
				monitor_from: trimmed || null,
				refresh_hours: refresh === 'default' ? null : refresh === 'never' ? 0 : refreshHours
			})
		);
	}

	function toggleAll(on: boolean) {
		selected = new Set(on ? visible.map((c) => c.id) : []);
	}

	function toggle(cid: number) {
		const next = new Set(selected);
		if (next.has(cid)) next.delete(cid);
		else next.add(cid);
		selected = next;
	}

	async function setMonitored(ids: number[], on: boolean) {
		await act(() => api.setMonitored(ids, on));
		selected = new Set();
	}

	function downloadWanted() {
		const ids = chapters.filter((c) => c.monitored && !c.file_path && !jobs.has(c.id));
		act(
			() => Promise.all(ids.map((c) => api.downloadChapter(c.id))),
			`Queued ${ids.length} chapter(s)`
		);
	}

	let unfollowDialog = $state<HTMLDialogElement>();
	let deleteFolder = $state(false);

	async function unfollow(event: SubmitEvent) {
		event.preventDefault();
		if (!series) return;
		try {
			await api.unfollow(id, deleteFolder);
			toasts.show(
				deleteFolder
					? `Unfollowed ${series.title} and deleted its files`
					: `Unfollowed ${series.title}`
			);
			goto('/');
		} catch (e) {
			toasts.error(e);
		}
	}

	async function deleteFiles(ids: number[]) {
		const n = ids.length;
		if (
			!n ||
			!confirm(
				`Delete ${n === 1 ? 'this file' : `${n} files`} from disk? The chapters won't be downloaded again.`
			)
		)
			return;
		await act(() => api.deleteFiles(ids), `Deleted ${n} file(s)`);
		selected = new Set();
	}

	const lowest = $derived(chapters.length ? chapters[chapters.length - 1].number : null);
	const allSelected = $derived(visible.length > 0 && visible.every((c) => selected.has(c.id)));
	const selectedOnDisk = $derived(
		chapters.filter((c) => c.file_path && selected.has(c.id)).map((c) => c.id)
	);
	const sortedSelection = $derived(
		[...selected].sort((a, b) => {
			const ca = chapters.find((c) => c.id === a)?.number ?? '0';
			const cb = chapters.find((c) => c.id === b)?.number ?? '0';
			return compareNumbers(ca, cb);
		})
	);
</script>

<svelte:head><title>{series?.title ?? 'Series'} · kaze</title></svelte:head>

<p class="m-0 text-sm text-muted">
	<a href="/" class="text-muted">Library</a> <span aria-hidden="true">/</span>
	{series?.title ?? '…'}
</p>

{#if error}
	<p class="card m-0 border-bad-line p-4 text-bad">Couldn't load this series: {error}</p>
{:else if series}
	<div class="flex flex-wrap items-start gap-8">
		<Cover
			id={series.id}
			title={series.title}
			size="lg"
			class="w-[160px] flex-none rounded-xl sm:w-[196px]"
		/>

		<section aria-label="About" class="flex min-w-0 flex-[999_1_420px] flex-col gap-3.5">
			<h1 class="m-0 font-display text-[30px] leading-[1.15] font-bold sm:text-[38px]">
				{series.title}
			</h1>
			<p class="m-0 flex flex-wrap gap-x-[18px] gap-y-1.5 text-ink-2">
				{#if series.authors.length}<span>{series.authors.join(', ')}</span>{/if}
				{#if series.year}<span>{series.year}</span>{/if}
				{#if series.status !== 'unknown'}<span class="capitalize">{series.status}</span>{/if}
				<a href={series.url} target="_blank" rel="noreferrer" class="text-ink-2 capitalize"
					>{series.source} ↗</a
				>
			</p>
			{#if series.tags.length}
				<div class="flex flex-wrap gap-2">
					{#each series.tags as tag (tag)}
						<span class="rounded-full bg-raised px-2.5 py-[3px] text-[13px] text-ink-2">{tag}</span>
					{/each}
				</div>
			{/if}
			{#if series.description}
				<p class="m-0 max-w-[72ch] whitespace-pre-line text-ink-2">{series.description}</p>
			{/if}
			<div class="flex flex-wrap gap-2.5 pt-1">
				<button
					type="button"
					class="btn-ghost"
					disabled={busy}
					onclick={() => act(() => api.refreshSeries(id), 'Checking for new chapters…')}
				>
					<Icon name="refresh" size={16} stroke={2} /> Check for chapters
				</button>
				<button
					type="button"
					class="btn-danger"
					onclick={() => {
						deleteFolder = false;
						unfollowDialog?.showModal();
					}}>Unfollow</button
				>
			</div>
			<p class="m-0 text-[13px] text-muted">
				Last checked {ago(series.last_checked_at)}
			</p>
		</section>

		<aside aria-label="Monitoring" class="card flex flex-[1_1_300px] flex-col gap-4 p-5">
			<h2 class="m-0 text-[15px] font-semibold">Monitoring</h2>
			<MonitorPicker name="monitor" legend="Which chapters to download" bind:value={monitor} />
			<label class="flex items-center justify-between gap-3 text-sm text-ink-2">
				Starting at chapter
				<input
					type="text"
					inputmode="decimal"
					placeholder={lowest ?? 'any'}
					bind:value={from}
					class="field-sm w-[90px]"
				/>
			</label>
			<label class="flex items-center justify-between gap-3 text-sm text-ink-2">
				Full refresh
				<select bind:value={refresh} class="field-sm w-auto font-sans">
					<option value="default">Global setting</option>
					<option value="custom">Every…</option>
					<option value="never">Never</option>
				</select>
			</label>
			{#if refresh === 'custom'}
				<label class="flex items-center justify-between gap-3 text-sm text-ink-2">
					Hours between refreshes
					<input type="number" min="1" bind:value={refreshHours} class="field-sm w-[90px]" />
				</label>
			{/if}
			<button type="button" class="btn-ghost" disabled={busy} onclick={saveMonitoring}>
				Save monitoring
			</button>
			<dl class="m-0 grid grid-cols-3 gap-2 text-center">
				<div class="rounded-[10px] bg-ground px-1 py-2.5">
					<dt class="text-xs text-muted">Chapters</dt>
					<dd class="m-0 font-mono text-lg">{num(stats.total)}</dd>
				</div>
				<div class="rounded-[10px] bg-ground px-1 py-2.5">
					<dt class="text-xs text-muted">On disk</dt>
					<dd class="m-0 font-mono text-lg text-disk">{num(stats.disk)}</dd>
				</div>
				<div class="rounded-[10px] bg-ground px-1 py-2.5">
					<dt class="text-xs text-muted">Wanted</dt>
					<dd class="m-0 font-mono text-lg text-wanted">{num(stats.wanted)}</dd>
				</div>
			</dl>
			<p class="m-0 font-mono text-xs break-all text-muted">{series.folder}/</p>
		</aside>
	</div>

	<section aria-labelledby="chapters-heading" class="flex flex-col gap-3.5">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<h2 id="chapters-heading" class="section-title">Chapters</h2>
			<div class="flex flex-wrap items-center gap-2.5">
				{#if selected.size}
					<span class="text-sm text-muted">{selected.size} selected</span>
					<button
						type="button"
						class="btn-ghost"
						disabled={busy}
						onclick={() => setMonitored(sortedSelection, true)}>Monitor</button
					>
					<button
						type="button"
						class="btn-ghost"
						disabled={busy}
						onclick={() => setMonitored(sortedSelection, false)}>Unmonitor</button
					>
					{#if selectedOnDisk.length}
						<button
							type="button"
							class="btn-danger"
							disabled={busy}
							onclick={() => deleteFiles(selectedOnDisk)}
							>Delete files ({num(selectedOnDisk.length)})</button
						>
					{/if}
				{/if}
				<Segmented
					label="Show"
					bind:value={show}
					options={[
						{ value: 'all', label: 'All' },
						{ value: 'wanted', label: 'Wanted' },
						{ value: 'disk', label: 'On disk' }
					]}
				/>
				<button
					type="button"
					class="btn-primary"
					disabled={busy || stats.wanted === 0}
					onclick={downloadWanted}>Download wanted ({num(stats.wanted)})</button
				>
			</div>
		</div>

		{#if chapters.length === 0}
			<p class="card m-0 p-5 text-muted">
				No chapters yet. kaze is reading the chapter list; this page updates when it's done.
			</p>
		{:else}
			<div class="overflow-x-auto rounded-[14px] border border-line">
				<table class="w-full min-w-[720px] border-collapse text-sm">
					<thead>
						<tr class="bg-surface text-left text-muted">
							<th scope="col" class="w-11 py-3 pl-4">
								<input
									type="checkbox"
									aria-label="Select all shown chapters"
									checked={allSelected}
									onchange={(e) => toggleAll(e.currentTarget.checked)}
								/>
							</th>
							<th scope="col" class="p-3 font-medium">Chapter</th>
							<th scope="col" class="p-3 font-medium">Title</th>
							<th scope="col" class="p-3 font-medium">Released</th>
							<th scope="col" class="p-3 font-medium">State</th>
							<th scope="col" class="px-4 py-3 text-right font-medium">Actions</th>
						</tr>
					</thead>
					<tbody>
						{#each visible as c (c.id)}
							{@const state = chapterState(c)}
							<tr class="border-t border-raised hover:bg-surface-2">
								<td class="py-2.5 pl-4">
									<input
										type="checkbox"
										aria-label="Select chapter {c.number}"
										checked={selected.has(c.id)}
										onchange={() => toggle(c.id)}
									/>
								</td>
								<td class="px-3 py-2.5 font-mono">{chapterName(c)}</td>
								<td class="px-3 py-2.5 {c.title ? 'text-ink' : 'text-faint'}">{c.title ?? '—'}</td>
								<td class="px-3 py-2.5 text-muted" title={c.published_at ?? ''}>
									{ago(c.published_at)}
								</td>
								<td class="px-3 py-2.5"><StateChip tone={state.tone} label={state.label} /></td>
								<td class="px-4 py-1.5 text-right whitespace-nowrap">
									<button
										type="button"
										class="icon-btn"
										aria-label="Download chapter {c.number}{c.file_path ? ' again' : ''}"
										title={c.file_path ? 'Download again' : 'Download now'}
										disabled={jobs.has(c.id)}
										onclick={() => act(() => api.downloadChapter(c.id))}
									>
										<Icon name="download" />
									</button>
									<button
										type="button"
										class="icon-btn {c.monitored ? 'text-seal-soft' : 'text-faint'}"
										aria-label="{c.monitored ? 'Stop monitoring' : 'Monitor'} chapter {c.number}"
										aria-pressed={c.monitored}
										title={c.monitored ? 'Monitored' : 'Not monitored'}
										onclick={() => setMonitored([c.id], !c.monitored)}
									>
										<Icon name="wanted" />
									</button>
									{#if c.file_path}
										<button
											type="button"
											class="icon-btn hover:text-bad"
											aria-label="Delete the file of chapter {c.number}"
											title="Delete file"
											disabled={busy}
											onclick={() => deleteFiles([c.id])}
										>
											<Icon name="trash" />
										</button>
									{/if}
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
			<p class="m-0 text-[13px] text-muted">
				Showing {num(visible.length)} of {num(filtered.length)} chapters
				{#if visible.length < filtered.length}
					·
					<button
						type="button"
						class="cursor-pointer border-0 bg-transparent p-0 text-ink-2 underline"
						onclick={() => (limit += PAGE * 4)}>show more</button
					>
					·
					<button
						type="button"
						class="cursor-pointer border-0 bg-transparent p-0 text-ink-2 underline"
						onclick={() => (limit = Infinity)}>show all</button
					>
				{/if}
			</p>
		{/if}
	</section>
{:else}
	<div class="h-72 animate-pulse rounded-[14px] bg-surface"></div>
{/if}

{#if series}
	<dialog
		bind:this={unfollowDialog}
		class="m-auto w-[min(480px,calc(100vw-24px))] rounded-2xl border border-line-strong bg-surface p-0 text-ink backdrop:bg-rail/80"
		aria-labelledby="unfollow-title"
	>
		<form class="flex flex-col gap-4 p-6" onsubmit={unfollow}>
			<h2 id="unfollow-title" class="m-0 font-display text-xl font-bold">
				Stop following {series.title}?
			</h2>
			<label class="flex items-start gap-3 text-sm">
				<input type="checkbox" bind:checked={deleteFolder} class="mt-0.5" />
				<span>
					Also delete the folder and its {num(stats.disk)} downloaded file(s)
					<code class="block font-mono text-xs break-all text-muted">{series.folder}/</code>
				</span>
			</label>
			<div class="flex justify-end gap-2.5">
				<button type="button" class="btn-ghost" onclick={() => unfollowDialog?.close()}
					>Cancel</button
				>
				<button type="submit" class="btn-danger"
					>{deleteFolder ? 'Unfollow and delete' : 'Unfollow'}</button
				>
			</div>
		</form>
	</dialog>
{/if}
