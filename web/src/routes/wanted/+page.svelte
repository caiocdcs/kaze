<script lang="ts">
	import { api, type WantedChapter } from '#lib/api/index.ts';
	import { chapterName } from '#lib/chapters.ts';
	import Icon from '#lib/components/Icon.svelte';
	import { ago, errorMessage, num } from '#lib/format.ts';
	import { live } from '#lib/live.svelte.ts';
	import { toasts } from '#lib/toast.svelte.ts';

	let wanted = $state<WantedChapter[] | null>(null);
	let error = $state<string | null>(null);
	let selected = $state<Set<number>>(new Set());
	let busy = $state(false);

	async function load() {
		try {
			wanted = await api.wanted();
			error = null;
			// Drop selections of chapters that are no longer wanted.
			const ids = new Set(wanted.map((c) => c.id));
			selected = new Set([...selected].filter((id) => ids.has(id)));
		} catch (e) {
			error = errorMessage(e);
		}
	}

	$effect(() => {
		void live.libraryVersion;
		void live.jobsVersion;
		load();
	});

	const groups = $derived.by(() => {
		const by = new Map<
			number,
			{ id: number; title: string; source: string; rows: WantedChapter[] }
		>();
		for (const c of wanted ?? []) {
			let g = by.get(c.series_id);
			if (!g) {
				g = { id: c.series_id, title: c.series_title, source: c.source, rows: [] };
				by.set(c.series_id, g);
			}
			g.rows.push(c);
		}
		return [...by.values()];
	});

	/** Chapter ids with a queued or running download. */
	const active = $derived(
		new Map(
			live.active
				.filter((j) => j.job.type === 'download_chapter')
				.map((j) => [(j.job as { chapter_id: number }).chapter_id, j])
		)
	);

	function attempt(c: WantedChapter): { text: string; tone: string; title?: string } {
		const job = active.get(c.id);
		if (job?.state === 'running') {
			const p = live.progress.get(job.id);
			return {
				text: p ? `Downloading ${p.done}/${p.total}` : 'Downloading',
				tone: 'text-seal-soft'
			};
		}
		if (job) return { text: 'Queued', tone: 'text-seal-soft' };
		if (!c.last_attempt_at) return { text: 'Never tried', tone: 'text-muted' };
		const when = ago(c.last_attempt_at);
		switch (c.last_attempt_state) {
			case 'failed':
				return { text: `Failed ${when}`, tone: 'text-bad', title: c.last_error ?? undefined };
			case 'cancelled':
				return { text: `Cancelled ${when}`, tone: 'text-muted' };
			default:
				return { text: when, tone: 'text-muted' };
		}
	}

	function toggle(ids: number[], on: boolean) {
		const next = new Set(selected);
		for (const id of ids) {
			if (on) next.add(id);
			else next.delete(id);
		}
		selected = next;
	}

	async function run(work: () => Promise<unknown>, done: string) {
		busy = true;
		try {
			await work();
			toasts.show(done);
			await load();
		} catch (e) {
			toasts.error(e);
		} finally {
			busy = false;
		}
	}

	const downloadAll = () =>
		run(async () => {
			const { queued } = await api.downloadWanted();
			return queued;
		}, 'Queued every wanted chapter');

	const downloadSelected = () => {
		const ids = [...selected];
		run(
			() => Promise.all(ids.map((id) => api.downloadChapter(id))),
			`Queued ${ids.length} chapter(s)`
		);
	};

	const unmonitorSelected = () => {
		const ids = [...selected];
		run(async () => {
			await api.setMonitored(ids, false);
			selected = new Set();
		}, `Stopped monitoring ${ids.length} chapter(s)`);
	};

	const allSelected = $derived(!!wanted?.length && wanted.every((c) => selected.has(c.id)));
</script>

<svelte:head><title>Wanted · kaze</title></svelte:head>

<header class="flex flex-wrap items-end justify-between gap-4">
	<div class="flex flex-col gap-1">
		<h1 class="page-title">Wanted</h1>
		{#if wanted}
			<p class="m-0 text-muted">
				Monitored chapters that aren’t on disk yet: {num(wanted.length)} across {num(groups.length)} series.
			</p>
		{/if}
	</div>
	<div class="flex flex-wrap gap-2.5">
		{#if selected.size}
			<button type="button" class="btn-ghost" disabled={busy} onclick={unmonitorSelected}>
				Stop monitoring selected
			</button>
			<button type="button" class="btn-ghost" disabled={busy} onclick={downloadSelected}>
				<Icon name="download" size={16} stroke={2.2} /> Download {num(selected.size)} selected
			</button>
		{/if}
		<button
			type="button"
			class="btn-primary"
			disabled={busy || !wanted?.length}
			onclick={downloadAll}
		>
			<Icon name="download" size={16} stroke={2.2} /> Download all {wanted?.length
				? num(wanted.length)
				: ''}
		</button>
	</div>
</header>

{#if error}
	<p class="card m-0 border-bad-line p-4 text-bad">Couldn't load the wanted list: {error}</p>
{:else if wanted === null}
	<div class="h-64 animate-pulse rounded-[14px] bg-surface"></div>
{:else if wanted.length === 0}
	<div class="card flex flex-col gap-2 p-8">
		<h2 class="section-title">All caught up</h2>
		<p class="m-0 text-ink-2">
			Every monitored chapter is on disk. New ones show up here when sources publish them.
		</p>
	</div>
{:else}
	<div class="overflow-x-auto rounded-[14px] border border-line">
		<table class="w-full min-w-[680px] border-collapse text-sm">
			<thead>
				<tr class="bg-surface text-left text-muted">
					<th scope="col" class="w-11 py-3 pl-4">
						<input
							type="checkbox"
							aria-label="Select every wanted chapter"
							checked={allSelected}
							onchange={(e) => toggle(wanted?.map((c) => c.id) ?? [], e.currentTarget.checked)}
						/>
					</th>
					<th scope="col" class="p-3 font-medium">Chapter</th>
					<th scope="col" class="p-3 font-medium">Released</th>
					<th scope="col" class="p-3 font-medium">Last attempt</th>
					<th scope="col" class="px-4 py-3 text-right font-medium">Actions</th>
				</tr>
			</thead>
			{#each groups as g (g.id)}
				{@const groupSelected = g.rows.every((c) => selected.has(c.id))}
				<tbody>
					<tr class="border-t border-line bg-[#13161A]">
						<td class="py-3 pl-4">
							<input
								type="checkbox"
								aria-label="Select every wanted chapter of {g.title}"
								checked={groupSelected}
								onchange={(e) =>
									toggle(
										g.rows.map((c) => c.id),
										e.currentTarget.checked
									)}
							/>
						</td>
						<th scope="rowgroup" colspan="4" class="px-3 py-3 text-left">
							<span class="flex items-center gap-3">
								<a href="/series/{g.id}" class="font-semibold no-underline">{g.title}</a>
								<span class="text-xs text-muted capitalize">{g.source}</span>
								<span class="rounded-full bg-wanted-wash px-2 py-px font-mono text-xs text-wanted"
									>{num(g.rows.length)}</span
								>
							</span>
						</th>
					</tr>
					{#each g.rows as c (c.id)}
						{@const a = attempt(c)}
						<tr class="border-t border-[#1B1F25] hover:bg-surface-2">
							<td class="py-2 pl-4">
								<input
									type="checkbox"
									aria-label="Select {g.title} chapter {c.number}"
									checked={selected.has(c.id)}
									onchange={(e) => toggle([c.id], e.currentTarget.checked)}
								/>
							</td>
							<td class="px-3 py-2">
								<span class="font-mono">{chapterName(c)}</span>
								{#if c.title}<span class="text-muted"> {c.title}</span>{/if}
							</td>
							<td class="px-3 py-2 text-muted">{ago(c.published_at)}</td>
							<td class="px-3 py-2 {a.tone}" title={a.title}>{a.text}</td>
							<td class="px-4 py-1 text-right whitespace-nowrap">
								<button
									type="button"
									class="icon-btn"
									aria-label="Download {g.title} chapter {c.number}"
									disabled={active.has(c.id)}
									onclick={() =>
										run(() => api.downloadChapter(c.id), `Queued ${g.title} ${chapterName(c)}`)}
								>
									<Icon name="download" />
								</button>
								<button
									type="button"
									class="icon-btn text-seal-soft"
									aria-label="Stop monitoring {g.title} chapter {c.number}"
									title="Stop monitoring"
									onclick={() =>
										run(
											() => api.setMonitored([c.id], false),
											`Stopped monitoring ${chapterName(c)}`
										)}
								>
									<Icon name="wanted" />
								</button>
							</td>
						</tr>
					{/each}
				</tbody>
			{/each}
		</table>
	</div>
{/if}
