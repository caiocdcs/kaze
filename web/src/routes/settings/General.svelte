<script lang="ts">
	import { api, type Settings, type SettingsView, type SourceInfo } from '#lib/api/index.ts';
	import { errorMessage, languageName, offeredLanguages } from '#lib/format.ts';
	import { toasts } from '#lib/toast.svelte.ts';

	type Key = keyof Settings;

	let view = $state<SettingsView | null>(null);
	let form = $state<Settings | null>(null);
	let error = $state<string | null>(null);
	let saving = $state(false);
	let sources = $state<SourceInfo[]>([]);
	const languages = $derived(offeredLanguages(sources));

	$effect(() => {
		api
			.sources()
			.then((s) => (sources = s))
			.catch(() => {});
		api
			.settings()
			.then(set)
			.catch((e) => (error = errorMessage(e)));
	});

	function set(v: SettingsView) {
		view = v;
		form = structuredClone($state.snapshot(v.settings));
	}

	const locked = (k: Key) => view?.locked.includes(k) ?? false;
	const changed = $derived.by(() => {
		if (!view || !form) return {};
		const out: Record<string, unknown> = {};
		for (const k of Object.keys(form) as Key[]) {
			// Set from their own tabs.
			if (['paused_sources', 'oidc', 'api_key', 'flaresolverr_url'].includes(k)) continue;
			const a = form[k] === '' ? null : form[k];
			if (!locked(k) && a !== view.settings[k]) out[k] = a;
		}
		return out;
	});
	const dirty = $derived(Object.keys(changed).length > 0);

	async function save(event: SubmitEvent) {
		event.preventDefault();
		saving = true;
		try {
			set(await api.updateSettings(changed as Parameters<typeof api.updateSettings>[0]));
			toasts.show('Settings saved');
		} catch (e) {
			toasts.error(e);
		} finally {
			saving = false;
		}
	}

	const numbers: { key: Key; label: string; hint: string; min: number; max: number }[] = [
		{
			key: 'concurrency',
			label: 'Jobs at once',
			hint: 'Downloads and refreshes running together.',
			min: 1,
			max: 32
		},
		{
			key: 'per_source',
			label: 'Jobs per site',
			hint: 'At once against one source; keep it low to be polite.',
			min: 1,
			max: 16
		},
		{
			key: 'page_concurrency',
			label: 'Pages at once',
			hint: 'Images of one chapter fetched together.',
			min: 1,
			max: 16
		},
		{
			key: 'sync_latest_minutes',
			label: 'Latest updates (minutes)',
			hint: 'How often each site’s “latest” page is checked for followed series.',
			min: 1,
			max: 10080
		},
		{
			key: 'refresh_hours',
			label: 'Full refresh (hours)',
			hint: 'How often each series’ whole chapter list is re-read. Series can override it.',
			min: 1,
			max: 2160
		},
		{
			key: 'backup_days',
			label: 'Back up every (days)',
			hint: 'A copy of the database in data/backups. 0 turns scheduled backups off.',
			min: 0,
			max: 365
		},
		{
			key: 'backup_keep',
			label: 'Backups kept',
			hint: 'Older backups are deleted after each new one.',
			min: 1,
			max: 100
		}
	];
</script>

{#if error}
	<p class="card m-0 border-bad-line p-4 text-bad">Couldn't load settings: {error}</p>
{:else if form && view}
	<form class="flex max-w-3xl flex-col gap-8 pt-6" onsubmit={save}>
		<section class="flex flex-col gap-4" aria-labelledby="files-heading">
			<h2 id="files-heading" class="section-title text-lg">Files</h2>
			<label class="flex flex-col gap-1.5">
				<span class="flex items-center gap-2 font-medium">
					Chapter file name
					{#if locked('naming_template')}<span class="lock">set in kaze.toml</span>{/if}
				</span>
				<input
					type="text"
					bind:value={form.naming_template}
					disabled={locked('naming_template')}
					class="field font-mono text-sm"
					spellcheck="false"
				/>
				<span class="text-[13px] text-muted">
					<code class="font-mono">%M</code> series, <code class="font-mono">%V</code> volume,
					<code class="font-mono">%C</code> chapter, <code class="font-mono">%T</code> title,
					<code class="font-mono">%A</code> author, <code class="font-mono">%Y</code> year.
					<code class="font-mono">?V(…)</code> keeps the part only when the volume is known.
				</span>
			</label>
			<label class="flex flex-col gap-1.5">
				<span class="flex items-center gap-2 font-medium">
					Default language
					{#if locked('default_language')}<span class="lock">set in kaze.toml</span>{/if}
				</span>
				<select
					bind:value={form.default_language}
					disabled={locked('default_language')}
					class="field w-64 font-sans"
				>
					{#each languages.includes(form.default_language) ? languages : [form.default_language, ...languages] as l (l)}
						<option value={l}>{languageName(l)}</option>
					{/each}
				</select>
				<span class="text-[13px] text-muted">
					What Add series searches and follows in. Sources without it use their own. Each language
					can have its own folder (<code class="font-mono">library_dirs</code> in kaze.toml).
				</span>
			</label>
		</section>

		<section class="flex flex-col gap-4" aria-labelledby="jobs-heading">
			<h2 id="jobs-heading" class="section-title text-lg">Downloads and schedule</h2>
			<div class="grid gap-x-6 gap-y-5 sm:grid-cols-2">
				{#each numbers as n (n.key)}
					<label class="flex flex-col gap-1.5">
						<span class="flex items-center gap-2 font-medium">
							{n.label}
							{#if locked(n.key)}<span class="lock">set in kaze.toml</span>{/if}
						</span>
						<input
							type="number"
							min={n.min}
							max={n.max}
							required
							bind:value={form[n.key]}
							disabled={locked(n.key)}
							class="field w-36 font-mono"
						/>
						<span class="text-[13px] text-muted">{n.hint}</span>
					</label>
				{/each}
			</div>
			<label class="flex items-start gap-3">
				<input
					type="checkbox"
					bind:checked={form.check_updates}
					disabled={locked('check_updates')}
					class="mt-1"
				/>
				<span class="flex flex-col gap-0.5">
					<span class="flex items-center gap-2 font-medium">
						Check for updates
						{#if locked('check_updates')}<span class="lock">set in kaze.toml</span>{/if}
					</span>
					<span class="text-[13px] text-muted"
						>Once a day, ask GitHub for the latest kaze release; the health panel says when one is
						out. Nothing but kaze's version is sent.</span
					>
				</span>
			</label>
		</section>

		<div class="sticky bottom-24 flex items-center gap-3 bg-ground py-3 md:bottom-0">
			<button type="submit" class="btn-primary" disabled={!dirty || saving}>
				{saving ? 'Saving…' : 'Save changes'}
			</button>
			{#if dirty}
				<button type="button" class="btn-ghost" onclick={() => view && set(view)}>Discard</button>
			{/if}
		</div>
	</form>
{:else}
	<div class="mt-6 h-64 max-w-3xl animate-pulse rounded-[14px] bg-surface"></div>
{/if}

<style>
	.lock {
		font-size: 12px;
		font-weight: 500;
		padding: 1px 8px;
		border-radius: 999px;
		background: var(--color-raised);
		color: var(--color-muted);
	}
</style>
