<script lang="ts">
	import { api, type TaskInfo } from '#lib/api/index.ts';
	import Icon from '#lib/components/Icon.svelte';
	import { ago, dateTime, errorMessage, every } from '#lib/format.ts';
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

<div class="flex max-w-3xl flex-col gap-4 pt-6">
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
</div>
