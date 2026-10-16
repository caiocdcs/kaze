<script lang="ts">
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import {
		api,
		type HistoryEntry,
		type JobRecord,
		type Settings,
		type SourceInfo,
		type TaskInfo
	} from '#lib/api/index.ts';
	import Icon from '#lib/components/Icon.svelte';
	import ProgressBar from '#lib/components/ProgressBar.svelte';
	import Segmented from '#lib/components/Segmented.svelte';
	import type { IconName } from '#lib/icons.ts';
	import { ago, dateTime, errorMessage, jobLabel, num } from '#lib/format.ts';
	import { live } from '#lib/live.svelte.ts';
	import { toasts } from '#lib/toast.svelte.ts';

	type Tab = 'queue' | 'history';
	type Kind = 'all' | 'downloaded' | 'failed' | 'found';

	const tab = $derived<Tab>(page.url.searchParams.get('tab') === 'history' ? 'history' : 'queue');
	let kind = $state<Kind>('all');

	let jobs = $state<JobRecord[]>([]);
	let history = $state<HistoryEntry[] | null>(null);
	let settings = $state<Settings | null>(null);
	let sources = $state<SourceInfo[]>([]);
	let tasks = $state<TaskInfo[]>([]);
	let error = $state<string | null>(null);

	$effect(() => {
		api.settings().then((v) => (settings = v.settings));
		api.sources().then((s) => (sources = s));
		api.tasks().then((t) => (tasks = t));
	});

	$effect(() => {
		void live.jobsVersion;
		api
			.jobs(300)
			.then((j) => {
				jobs = j;
				error = null;
			})
			.catch((e) => (error = errorMessage(e)));
	});

	$effect(() => {
		if (tab !== 'history') return;
		void live.libraryVersion;
		api
			.history(300)
			.then((h) => (history = h))
			.catch((e) => (error = errorMessage(e)));
	});

	const running = $derived(jobs.filter((j) => j.state === 'running'));
	const waiting = $derived(
		jobs
			.filter((j) => j.state === 'queued')
			.sort((a, b) => a.run_after.localeCompare(b.run_after) || a.id - b.id)
	);
	// Only recent failures: a failure that was retried later shows as a newer job.
	const failed = $derived(jobs.filter((j) => j.state === 'failed').slice(0, 20));

	const kindLabel: Record<JobRecord['job']['type'], string> = {
		download_chapter: 'Download',
		refresh_series: 'Refresh',
		sync_latest: 'Latest',
		notify: 'Notify',
		scan_library: 'Scan'
	};
	const sourceOf = (j: JobRecord) => ('source' in j.job ? j.job.source : null);
	const sourceName = (id: string) => sources.find((s) => s.id === id)?.name ?? id;

	/** Slots in use: overall and per source, against the configured limits. */
	const lanes = $derived.by(() => {
		if (!settings) return [];
		const lanes = [{ name: 'All jobs', used: running.length, size: settings.concurrency }];
		for (const s of sources) {
			lanes.push({
				name: s.name,
				used: running.filter((j) => sourceOf(j) === s.id).length,
				size: settings.per_source
			});
		}
		return lanes;
	});

	const nextSync = $derived(tasks.find((t) => t.task === 'sync_latest')?.next_run ?? null);

	const shownHistory = $derived((history ?? []).filter((e) => kind === 'all' || e.kind === kind));
	const historyLook: Record<string, { icon: IconName; tone: string }> = {
		downloaded: { icon: 'check', tone: 'text-disk bg-disk-wash' },
		failed: { icon: 'close', tone: 'text-bad bg-bad-wash' },
		found: { icon: 'add', tone: 'text-wanted bg-wanted-wash' },
		deleted: { icon: 'trash', tone: 'text-muted bg-raised' },
		source_broken: { icon: 'close', tone: 'text-bad bg-bad-wash' }
	};

	async function act(work: () => Promise<unknown>, done?: string) {
		try {
			await work();
			if (done) toasts.show(done);
			jobs = await api.jobs(300);
		} catch (e) {
			toasts.error(e);
		}
	}

	function setTab(next: Tab) {
		goto(next === 'history' ? '?tab=history' : '?', { replace: true, reset: false });
	}

	function waitLabel(j: JobRecord): string {
		const due = new Date(j.run_after).getTime();
		if (due > Date.now() + 1000)
			return j.attempts > 0 ? `retry ${ago(j.run_after)}` : ago(j.run_after);
		return 'next up';
	}
</script>

<svelte:head><title>Activity · kaze</title></svelte:head>

<header class="flex flex-wrap items-end justify-between gap-4">
	<div class="flex flex-col gap-1">
		<h1 class="page-title">Activity</h1>
		<p class="m-0 flex items-center gap-2 text-muted">
			<span aria-hidden="true" class="size-2 rounded-full {live.connected ? 'bg-ok' : 'bg-bad'}"
			></span>
			{live.connected ? 'Live' : 'Reconnecting'}{nextSync
				? ` · next latest-updates check ${ago(nextSync)}`
				: ''}
		</p>
	</div>
	<div
		role="tablist"
		aria-label="Activity views"
		class="flex gap-1 rounded-[10px] border border-line-strong bg-surface p-1"
	>
		<button
			type="button"
			role="tab"
			aria-selected={tab === 'queue'}
			onclick={() => setTab('queue')}
			class="flex h-9 cursor-pointer items-center gap-2 rounded-[7px] border-0 px-4 {tab === 'queue'
				? 'bg-line-strong text-ink'
				: 'bg-transparent text-muted hover:text-ink'}"
		>
			Queue
			<span class="rounded-full bg-raised px-2 font-mono text-xs"
				>{num(running.length + waiting.length)}</span
			>
		</button>
		<button
			type="button"
			role="tab"
			aria-selected={tab === 'history'}
			onclick={() => setTab('history')}
			class="h-9 cursor-pointer rounded-[7px] border-0 px-4 {tab === 'history'
				? 'bg-line-strong text-ink'
				: 'bg-transparent text-muted hover:text-ink'}">History</button
		>
	</div>
</header>

{#if error}
	<p class="card m-0 border-bad-line p-4 text-bad">{error}</p>
{/if}

{#if tab === 'queue'}
	{#if lanes.length}
		<div class="flex flex-wrap gap-x-7 gap-y-3">
			{#each lanes as lane (lane.name)}
				<div class="flex items-center gap-2.5 text-sm">
					<span class="text-ink-2">{lane.name}</span>
					<span class="flex gap-1" aria-hidden="true">
						{#each { length: lane.size }, i (i)}
							<span class="h-2.5 w-4 rounded-sm {i < lane.used ? 'bg-seal' : 'bg-track'}"></span>
						{/each}
					</span>
					<span class="font-mono text-xs text-muted">{lane.used}/{lane.size}</span>
				</div>
			{/each}
		</div>
	{/if}

	<section aria-labelledby="running" class="flex flex-col gap-3">
		<h2 id="running" class="section-title text-lg">Running</h2>
		{#if running.length === 0}
			<p class="m-0 text-muted">Nothing running right now.</p>
		{:else}
			<div class="grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-3">
				{#each running as j (j.id)}
					{@const p = live.progress.get(j.id)}
					<article class="card flex flex-col gap-3 p-4">
						<div class="flex items-start justify-between gap-3">
							<div class="min-w-0">
								<p class="m-0 text-xs text-muted">
									{kindLabel[j.job.type]}{sourceOf(j) ? ` · ${sourceName(sourceOf(j) ?? '')}` : ''}
								</p>
								<p class="m-0 truncate font-semibold">
									{#if j.series_id !== null}
										<a href="/series/{j.series_id}" class="no-underline">{jobLabel(j)}</a>
									{:else}
										{jobLabel(j)}
									{/if}
								</p>
							</div>
							<button
								type="button"
								class="icon-btn -mt-1 -mr-1 size-9"
								aria-label="Cancel {jobLabel(j)}"
								onclick={() => act(() => api.cancelJob(j.id), 'Cancelled')}
								><Icon name="close" size={16} stroke={2} /></button
							>
						</div>
						<ProgressBar
							value={p ? p.done / Math.max(1, p.total) : 0}
							label="Progress of {jobLabel(j)}"
						/>
						<p class="m-0 flex justify-between text-xs text-muted">
							<span
								>{j.attempts > 1
									? `Attempt ${j.attempts} of ${j.max_attempts}`
									: `Started ${ago(j.updated_at)}`}</span
							>
							<span class="font-mono">{p ? `${p.done}/${p.total} pages` : ''}</span>
						</p>
					</article>
				{/each}
			</div>
		{/if}
	</section>

	<section aria-labelledby="waiting" class="flex flex-col gap-3">
		<h2 id="waiting" class="section-title text-lg">
			Waiting <span class="font-sans text-base font-normal text-muted">· {num(waiting.length)}</span
			>
		</h2>
		{#if waiting.length === 0}
			<p class="m-0 text-muted">The queue is empty.</p>
		{:else}
			<ul class="m-0 flex list-none flex-col overflow-hidden rounded-[14px] border border-line p-0">
				{#each waiting.slice(0, 100) as j (j.id)}
					<li class="flex items-center gap-3 border-t border-line px-4 py-1.5 first:border-t-0">
						<span class="w-20 shrink-0 text-xs text-muted">{kindLabel[j.job.type]}</span>
						<span class="min-w-0 flex-1 truncate">{jobLabel(j)}</span>
						<span
							class="text-xs whitespace-nowrap {j.attempts > 0 ? 'text-wanted' : 'text-muted'}"
							title={j.last_error ?? undefined}>{waitLabel(j)}</span
						>
						<button
							type="button"
							class="icon-btn size-9"
							aria-label="Cancel {jobLabel(j)}"
							onclick={() => act(() => api.cancelJob(j.id))}
							><Icon name="close" size={16} stroke={2} /></button
						>
					</li>
				{/each}
				{#if waiting.length > 100}
					<li class="border-t border-line px-4 py-2.5 text-sm text-muted">
						…and {num(waiting.length - 100)} more
					</li>
				{/if}
			</ul>
		{/if}
	</section>

	{#if failed.length}
		<section aria-labelledby="failed" class="flex flex-col gap-3">
			<h2 id="failed" class="section-title text-lg">Gave up</h2>
			<div class="flex flex-col gap-2">
				{#each failed as j (j.id)}
					<div
						class="flex flex-wrap items-center gap-3 rounded-xl border border-bad-line bg-bad-wash/40 px-4 py-2.5"
					>
						<span class="rounded-full bg-bad-wash px-2.5 py-0.5 text-xs font-medium text-bad"
							>Failed</span
						>
						<span class="min-w-0 flex-1">
							{jobLabel(j)}
							<span class="text-sm text-muted"
								>— {j.last_error ?? 'unknown error'} after {j.attempts} attempt{j.attempts === 1
									? ''
									: 's'}</span
							>
						</span>
						<button
							type="button"
							class="btn-ghost h-10"
							onclick={() => act(() => api.retryJob(j.id), 'Queued again')}
							><Icon name="refresh" size={16} /> Retry</button
						>
					</div>
				{/each}
			</div>
		</section>
	{/if}
{:else}
	<section aria-label="History" class="flex flex-col gap-4">
		<Segmented
			label="Show"
			bind:value={kind}
			options={[
				{ value: 'all', label: 'Everything' },
				{ value: 'downloaded', label: 'Downloaded' },
				{ value: 'failed', label: 'Failed' },
				{ value: 'found', label: 'New chapters' }
			]}
		/>
		{#if history === null}
			<div class="h-64 animate-pulse rounded-[14px] bg-surface"></div>
		{:else if shownHistory.length === 0}
			<p class="m-0 text-muted">Nothing here yet.</p>
		{:else}
			<ol class="m-0 flex list-none flex-col overflow-hidden rounded-[14px] border border-line p-0">
				{#each shownHistory as e (e.id)}
					{@const look = historyLook[e.kind] ?? { icon: 'activity', tone: 'text-muted bg-raised' }}
					<li class="flex items-center gap-3.5 border-t border-line px-4 py-3 first:border-t-0">
						<span class="flex size-8 shrink-0 items-center justify-center rounded-full {look.tone}">
							<Icon name={look.icon} size={16} stroke={2} />
						</span>
						<span class="min-w-0 flex-1">
							{#if e.series_id !== null}
								<a href="/series/{e.series_id}" class="no-underline">{e.message}</a>
							{:else}
								{e.message}
							{/if}
						</span>
						<time
							datetime={e.at}
							title={dateTime(e.at)}
							class="text-xs whitespace-nowrap text-muted">{ago(e.at)}</time
						>
					</li>
				{/each}
			</ol>
		{/if}
	</section>
{/if}
