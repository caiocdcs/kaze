<script lang="ts">
	import { api, type Config, type Integration, type Preset } from '#lib/api/index.ts';
	import HealthPanel from '#lib/components/HealthPanel.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import IntegrationEditor, {
		blank,
		kindLabel,
		type Draft
	} from '#lib/components/IntegrationEditor.svelte';
	import { ago, errorMessage } from '#lib/format.ts';
	import { toasts } from '#lib/toast.svelte.ts';

	let integrations = $state<Integration[] | null>(null);
	let presets = $state<Preset[]>([]);
	let error = $state<string | null>(null);
	let editing = $state<{ draft: Draft; hint?: string } | null>(null);
	let picking = $state(false);
	let version = $state(0);

	async function load() {
		try {
			integrations = await api.integrations();
			error = null;
			version++;
		} catch (e) {
			error = errorMessage(e);
		}
	}

	$effect(() => {
		load();
		api.presets().then((p) => (presets = p));
	});

	const tints: Record<Config['kind'], string> = {
		komga: '#2C3A52',
		kavita: '#4A3326',
		webhook: '#2E4A3C',
		apprise: '#3B3222'
	};

	function describe(i: Integration): string {
		const c = i.config;
		if (c.kind === 'webhook') {
			const preset = presets.find(
				(p) => p.config.kind === 'webhook' && new URL(p.config.url).host === new URL(c.url).host
			);
			return preset ? `Webhook · ${preset.name} preset` : 'Webhook';
		}
		return c.kind === 'apprise' ? 'Apprise' : `Library server · ${kindLabel[c.kind]}`;
	}

	function target(c: Config): string {
		try {
			const u = new URL(c.url);
			return u.origin + (u.pathname === '/' ? '' : u.pathname.replace(/\/[^/]{20,}/g, '/…'));
		} catch {
			return c.url;
		}
	}

	const eventLabels = {
		downloaded: 'Downloaded',
		failed: 'Failed',
		found: 'New chapters'
	} as const;

	function startNew(kind: Config['kind'], preset?: Preset) {
		picking = false;
		editing = {
			draft: {
				name: preset?.name ?? kindLabel[kind],
				enabled: true,
				events:
					preset?.events ?? (kind === 'komga' || kind === 'kavita' ? [] : ['downloaded', 'failed']),
				config: preset ? structuredClone(preset.config) : blank(kind)
			},
			hint: preset?.hint
		};
	}

	function edit(i: Integration) {
		editing = {
			draft: { id: i.id, name: i.name, enabled: i.enabled, events: i.events, config: i.config }
		};
	}

	async function toggle(i: Integration, enabled: boolean) {
		try {
			await api.replaceIntegration(i.id, {
				name: i.name,
				enabled,
				events: i.events,
				config: i.config
			});
			await load();
		} catch (e) {
			toasts.error(e);
		}
	}

	async function test(i: Integration) {
		try {
			await api.testIntegration(i.id);
			toasts.show(
				i.config.kind === 'komga' || i.config.kind === 'kavita'
					? `${i.name}: connected`
					: `${i.name}: test sent`
			);
		} catch (e) {
			toasts.error(e);
		}
		await load();
	}

	const pitches: { kind: Config['kind']; text: string }[] = [
		{ kind: 'komga', text: 'Scan your Komga library when new chapters arrive.' },
		{ kind: 'kavita', text: 'Scan your Kavita library when new chapters arrive.' },
		{
			kind: 'webhook',
			text: 'POST a JSON body you template: ntfy, Discord, Gotify, Home Assistant, n8n or your own scripts. Nothing else to run.'
		},
		{
			kind: 'apprise',
			text: 'If you run apprise-api, reach Telegram, email, Matrix and 100+ more.'
		}
	];
	const unset = $derived(
		pitches.filter(
			(p) => p.kind === 'webhook' || !integrations?.some((i) => i.config.kind === p.kind)
		)
	);
</script>

<div class="flex flex-wrap items-start gap-7 pt-6">
	<div class="flex min-w-0 flex-[999_1_560px] flex-col gap-5">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<p class="m-0 max-w-[60ch] text-ink-2">
				Tell your library server when chapters land, and get notified about downloads and problems.
			</p>
			<div class="relative">
				<button
					type="button"
					class="btn-primary"
					aria-expanded={picking}
					onclick={() => (picking = !picking)}
				>
					<Icon name="add" size={16} stroke={2.4} /> Add connection
				</button>
				{#if picking}
					<ul
						class="card absolute right-0 z-10 m-0 mt-2 flex w-60 list-none flex-col p-1.5 shadow-xl"
					>
						{#each [['komga', 'Komga'], ['kavita', 'Kavita'], ['apprise', 'Apprise'], ['webhook', 'Custom webhook']] as const as [kind, label] (kind)}
							<li>
								<button type="button" class="menu-item" onclick={() => startNew(kind)}
									>{label}</button
								>
							</li>
						{/each}
						<li class="px-3 pt-2 pb-1 text-xs text-muted">Webhook presets</li>
						{#each presets as p (p.id)}
							<li>
								<button type="button" class="menu-item" onclick={() => startNew('webhook', p)}
									>{p.name}</button
								>
							</li>
						{/each}
					</ul>
				{/if}
			</div>
		</div>

		{#if error}
			<p class="card m-0 border-bad-line p-4 text-bad">{error}</p>
		{/if}

		<div class="grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-3.5">
			{#each integrations ?? [] as i (i.id)}
				<article class="card flex flex-col gap-3.5 p-[18px]">
					<div class="flex items-center gap-3">
						<span
							class="flex size-10 flex-none items-center justify-center rounded-[10px] font-display text-lg font-bold text-paper"
							style:background={tints[i.config.kind]}
							aria-hidden="true">{i.name[0]?.toUpperCase()}</span
						>
						<div class="min-w-0 flex-1">
							<h2 class="m-0 truncate text-[15px] font-semibold">{i.name}</h2>
							<p class="m-0 truncate text-[13px] text-muted">{describe(i)}</p>
						</div>
						<label class="flex items-center gap-1.5 text-sm">
							<input
								type="checkbox"
								role="switch"
								checked={i.enabled}
								onchange={(e) => toggle(i, e.currentTarget.checked)}
							/>
							On
						</label>
					</div>
					<p class="m-0 truncate font-mono text-xs text-muted" title={i.config.url}>
						{target(i.config)}
					</p>
					{#if i.config.kind === 'komga' || i.config.kind === 'kavita'}
						<div class="flex flex-wrap gap-1.5">
							<span class="rounded-full bg-raised px-2.5 py-0.5 text-xs"
								>✓ Scan after downloads</span
							>
						</div>
					{:else}
						<div class="flex flex-wrap gap-1.5">
							{#each Object.entries(eventLabels) as [value, label] (value)}
								{@const on = i.events.includes(value as keyof typeof eventLabels)}
								<span
									class="rounded-full px-2.5 py-0.5 text-xs {on
										? 'bg-raised text-ink'
										: 'border border-line-strong text-faint'}">{on ? '✓ ' : ''}{label}</span
								>
							{/each}
						</div>
					{/if}
					<p
						class="m-0 flex items-center gap-2 text-[13px] text-muted"
						title={i.last_status ?? undefined}
					>
						<span
							aria-hidden="true"
							class="size-2 flex-none rounded-full {!i.enabled
								? 'bg-faint'
								: i.last_ok === false
									? 'bg-bad'
									: i.last_ok
										? 'bg-ok'
										: 'bg-faint'}"
						></span>
						<span class="truncate">
							{#if !i.enabled}
								Paused
							{:else if i.last_sent_at}
								{i.last_status} · {ago(i.last_sent_at)}
							{:else}
								Not used yet
							{/if}
						</span>
					</p>
					<div class="mt-auto flex gap-2">
						<button type="button" class="btn-ghost h-10 flex-1" onclick={() => test(i)}>
							{i.config.kind === 'komga' || i.config.kind === 'kavita' ? 'Test' : 'Send test'}
						</button>
						<button type="button" class="btn-ghost h-10 flex-1" onclick={() => edit(i)}>Edit</button
						>
					</div>
				</article>
			{/each}

			{#each unset as p (p.kind)}
				<article
					class="flex flex-col gap-3.5 rounded-[14px] border border-dashed border-line-strong p-[18px]"
				>
					<div class="flex items-center gap-3">
						<span
							class="flex size-10 flex-none items-center justify-center rounded-[10px] font-display text-lg font-bold text-paper"
							style:background={tints[p.kind]}
							aria-hidden="true">{kindLabel[p.kind][0]}</span
						>
						<div>
							<h2 class="m-0 text-[15px] font-semibold">{kindLabel[p.kind]}</h2>
							<p class="m-0 text-[13px] text-muted">
								{p.kind === 'webhook'
									? 'Built in · presets for ntfy, Discord, Gotify'
									: p.kind === 'apprise'
										? 'Notifications to 100+ services'
										: 'Library server'}
							</p>
						</div>
					</div>
					<p class="m-0 text-sm text-ink-2">{p.text}</p>
					<button
						type="button"
						class="btn-ghost mt-auto h-10"
						onclick={() => (p.kind === 'webhook' ? (picking = true) : startNew(p.kind))}
					>
						Set up
					</button>
				</article>
			{/each}
		</div>
	</div>

	<div class="flex flex-[1_1_300px] flex-col gap-4">
		<HealthPanel {version} />
	</div>
</div>

{#if editing}
	<IntegrationEditor
		draft={editing.draft}
		hint={editing.hint}
		onclose={() => (editing = null)}
		onsaved={() => {
			editing = null;
			load();
		}}
	/>
{/if}

<style>
	.menu-item {
		display: block;
		width: 100%;
		padding: 8px 12px;
		border: 0;
		border-radius: 8px;
		background: transparent;
		color: var(--color-ink);
		text-align: left;
		cursor: pointer;
	}
	.menu-item:hover {
		background: var(--color-raised);
	}
</style>
