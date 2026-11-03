<script lang="ts">
	import { untrack } from 'svelte';
	import { api, type BackupFile, type TaskInfo } from '#lib/api/index.ts';
	import HealthPanel from '#lib/components/HealthPanel.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import { ago, bytes, dateTime, errorMessage, every } from '#lib/format.ts';
	import { live } from '#lib/live.svelte.ts';
	import { toasts } from '#lib/toast.svelte.ts';

	let tasks = $state<TaskInfo[] | null>(null);
	let error = $state<string | null>(null);

	async function load() {
		try {
			tasks = await api.tasks();
			error = null;
		} catch (e) {
			error = errorMessage(e);
		}
	}

	$effect(() => {
		load();
		const timer = setInterval(load, 15_000);
		return () => clearInterval(timer);
	});

	let backingUp = $state(false);
	let backups = $state<BackupFile[]>([]);

	// A backup job finishing moves jobsVersion; the list follows.
	$effect(() => {
		void live.jobsVersion;
		untrack(() => api.backups().then((b) => (backups = b), toasts.error));
	});

	async function backup() {
		backingUp = true;
		try {
			await api.backup();
		} catch (e) {
			toasts.error(e);
		} finally {
			backingUp = false;
		}
	}

	async function backUpNow() {
		try {
			await api.runTask('backup');
			toasts.show('Backing up the database…');
		} catch (e) {
			toasts.error(e);
		}
	}

	async function remove(b: BackupFile) {
		if (!confirm(`Delete the backup from ${dateTime(b.created_at)}?`)) return;
		try {
			await api.deleteBackup(b.name);
			backups = backups.filter((x) => x.name !== b.name);
		} catch (e) {
			toasts.error(e);
		}
	}

	async function run(t: TaskInfo) {
		try {
			await api.runTask(t.task);
			toasts.show(`${t.name}: started`);
			setTimeout(load, 1000);
		} catch (e) {
			toasts.error(e);
		}
	}
</script>

<div class="flex flex-wrap items-start gap-7 pt-6">
	<div class="flex min-w-0 flex-[999_1_560px] flex-col gap-4">
		<section aria-labelledby="tasks-heading" class="flex flex-col gap-3">
			<h2 id="tasks-heading" class="section-title text-lg">Scheduled tasks</h2>
			{#if error}
				<p class="card m-0 border-bad-line p-4 text-bad">{error}</p>
			{:else if tasks}
				<div class="overflow-x-auto rounded-[14px] border border-line">
					<table class="w-full min-w-[560px] border-collapse text-sm">
						<thead>
							<tr class="bg-surface text-left text-muted">
								<th scope="col" class="p-3 pl-4 font-medium">Task</th>
								<th scope="col" class="p-3 font-medium">Interval</th>
								<th scope="col" class="p-3 font-medium">Last run</th>
								<th scope="col" class="p-3 font-medium">Next run</th>
								<th scope="col" class="p-3 pr-4"><span class="sr-only">Actions</span></th>
							</tr>
						</thead>
						<tbody>
							{#each tasks as t (t.task)}
								<tr class="border-t border-raised">
									<td class="p-3 pl-4 font-medium">{t.name}</td>
									<td class="p-3 text-muted">{every(t.every_secs)}</td>
									<td class="p-3 text-muted" title={dateTime(t.last_run)}>{ago(t.last_run)}</td>
									<td class="p-3 text-muted" title={dateTime(t.next_run)}>{ago(t.next_run)}</td>
									<td class="p-2 pr-4 text-right">
										<button type="button" class="btn-ghost h-10" onclick={() => run(t)}>
											<Icon name="play" size={14} /> Run now
										</button>
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{/if}
		</section>
		<section aria-labelledby="backup-heading" class="flex flex-col gap-3">
			<h2 id="backup-heading" class="section-title text-lg">Backup</h2>
			<div class="card flex flex-col gap-3 p-4">
				<div class="flex flex-wrap items-center justify-between gap-3">
					<p class="m-0 text-sm text-muted">
						Copies of the database: followed series, chapters, settings and integrations, kept in
						<code>data/backups</code>. Library files are not included. The schedule is under
						General.
					</p>
					<div class="flex gap-2">
						<button type="button" class="btn-ghost h-10" onclick={backUpNow}>
							<Icon name="play" size={14} /> Back up now
						</button>
						<button type="button" class="btn-ghost h-10" disabled={backingUp} onclick={backup}>
							<Icon name="download" size={14} />
							{backingUp ? 'Preparing…' : 'Download a fresh copy'}
						</button>
					</div>
				</div>
				{#if backups.length === 0}
					<p class="m-0 text-sm text-muted">No saved backups yet.</p>
				{:else}
					<ul class="m-0 flex list-none flex-col gap-1.5 p-0">
						{#each backups as b (b.name)}
							<li
								class="flex items-center gap-3 rounded-[10px] bg-ground py-1.5 pr-1.5 pl-3 text-sm"
							>
								<span class="flex-1" title={b.name}>{dateTime(b.created_at)}</span>
								<span class="text-muted">{ago(b.created_at)}</span>
								<span class="w-20 text-right font-mono text-xs text-muted"
									>{bytes(b.size_bytes)}</span
								>
								<button
									type="button"
									class="icon-btn"
									aria-label="Download the backup from {dateTime(b.created_at)}"
									title="Download"
									onclick={() => api.saveBackup(b.name).catch(toasts.error)}
								>
									<Icon name="download" />
								</button>
								<button
									type="button"
									class="icon-btn hover:text-bad"
									aria-label="Delete the backup from {dateTime(b.created_at)}"
									title="Delete"
									onclick={() => remove(b)}
								>
									<Icon name="trash" />
								</button>
							</li>
						{/each}
					</ul>
				{/if}
			</div>
		</section>
	</div>
	<div class="flex-[1_1_300px]"><HealthPanel /></div>
</div>
