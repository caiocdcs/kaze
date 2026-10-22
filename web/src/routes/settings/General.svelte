<script lang="ts">
	import {
		api,
		apiKey,
		setApiKey,
		type Settings,
		type SettingsView,
		type SourceInfo
	} from '#lib/api/index.ts';
	import { errorMessage, languageName, offeredLanguages } from '#lib/format.ts';
	import { toasts } from '#lib/toast.svelte.ts';

	type Key = keyof Settings;

	let view = $state<SettingsView | null>(null);
	let form = $state<Settings | null>(null);
	let error = $state<string | null>(null);
	let saving = $state(false);
	let revealKey = $state(false);
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
			// Set from the Sources tab.
			if (k === 'paused_sources') continue;
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
			const keyChanged = 'api_key' in changed;
			set(await api.updateSettings(changed as Parameters<typeof api.updateSettings>[0]));
			// Keep this browser working when the key it needs just changed.
			if (keyChanged) setApiKey(view?.settings.api_key ?? null);
			toasts.show('Settings saved');
		} catch (e) {
			toasts.error(e);
		} finally {
			saving = false;
		}
	}

	function generateKey() {
		if (!form) return;
		const bytes = crypto.getRandomValues(new Uint8Array(18));
		form.api_key = 'kz_' + [...bytes].map((b) => b.toString(16).padStart(2, '0')).join('');
		revealKey = true;
	}

	async function copyKey() {
		if (!form?.api_key) return;
		try {
			await navigator.clipboard.writeText(form.api_key);
			toasts.show('API key copied');
		} catch {
			toasts.show('Copy failed; select the key and copy it by hand', 'error');
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
		</section>

		<section class="flex flex-col gap-4" aria-labelledby="cf-heading">
			<h2 id="cf-heading" class="section-title text-lg">Cloudflare</h2>
			<label class="flex flex-col gap-1.5">
				<span class="flex items-center gap-2 font-medium">
					FlareSolverr URL
					{#if locked('flaresolverr_url')}<span class="lock">set in kaze.toml</span>{/if}
				</span>
				<input
					type="url"
					bind:value={form.flaresolverr_url}
					disabled={locked('flaresolverr_url')}
					placeholder="http://flaresolverr:8191"
					class="field max-w-md font-mono text-sm"
				/>
				<span class="text-[13px] text-muted">
					Optional. When a site answers with a Cloudflare challenge, kaze asks
					<a href="https://github.com/FlareSolverr/FlareSolverr" target="_blank" rel="noreferrer"
						>FlareSolverr</a
					> to get through, then keeps the cookie it earns.
				</span>
			</label>
		</section>

		<section class="flex flex-col gap-4" aria-labelledby="access-heading">
			<h2 id="access-heading" class="section-title text-lg">Access</h2>
			<div class="flex flex-col gap-1.5">
				<label for="api-key" class="flex items-center gap-2 font-medium">
					API key
					{#if locked('api_key')}<span class="lock">set in kaze.toml</span>{/if}
				</label>
				<div class="flex flex-wrap gap-2">
					<input
						id="api-key"
						type={revealKey ? 'text' : 'password'}
						bind:value={form.api_key}
						disabled={locked('api_key')}
						placeholder="Not required"
						autocomplete="off"
						class="field min-w-0 flex-[1_1_280px] font-mono text-sm"
					/>
					<button type="button" class="btn-ghost" onclick={() => (revealKey = !revealKey)}>
						{revealKey ? 'Hide' : 'Show'}
					</button>
					<button type="button" class="btn-ghost" onclick={copyKey} disabled={!form.api_key}
						>Copy</button
					>
					{#if !locked('api_key')}
						<button type="button" class="btn-ghost" onclick={generateKey}>Generate</button>
					{/if}
				</div>
				<span class="text-[13px] text-muted">
					When set, every API request must send it as <code class="font-mono">X-Api-Key</code>.
					Leave empty on a trusted LAN. {#if apiKey()}This browser has a key saved.{/if}
				</span>
			</div>
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
