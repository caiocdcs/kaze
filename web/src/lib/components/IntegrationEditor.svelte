<script lang="ts" module>
	import type { Config, IntegrationInput, Trigger } from '#lib/api/index.ts';

	export type Draft = IntegrationInput & { id?: number };

	export const kindLabel: Record<Config['kind'], string> = {
		komga: 'Komga',
		kavita: 'Kavita',
		ntfy: 'ntfy',
		gotify: 'Gotify',
		webhook: 'Webhook',
		apprise: 'Apprise'
	};

	export function blank(kind: Config['kind']): Config {
		switch (kind) {
			case 'komga':
				return { kind, url: 'http://komga:25600', api_key: '', library_id: null };
			case 'kavita':
				return { kind, url: 'http://kavita:5000', api_key: '', library_id: null };
			case 'ntfy':
				return {
					kind,
					url: 'https://ntfy.sh',
					topic: '',
					token: null,
					username: null,
					password: null,
					click_url: location.origin
				};
			case 'gotify':
				return { kind, url: 'https://gotify.example.com', token: '' };
			case 'webhook':
				return {
					kind,
					url: 'https://',
					method: 'POST',
					headers: {},
					body: '{\n  "title": {{ title | tojson }},\n  "message": {{ message | tojson }}\n}'
				};
			case 'apprise':
				return { kind, url: 'http://apprise:8000', key: null, urls: '', tags: null };
		}
	}
</script>

<script lang="ts">
	import { api } from '#lib/api/index.ts';
	import Segmented from '#lib/components/Segmented.svelte';
	import { errorMessage } from '#lib/format.ts';
	import { toasts } from '#lib/toast.svelte.ts';

	let {
		draft: initial,
		hint,
		onclose,
		onsaved
	}: { draft: Draft; hint?: string; onclose: () => void; onsaved: () => void } = $props();

	// The form edits a copy; the caller's object stays untouched until saved.
	// svelte-ignore state_referenced_locally
	let draft = $state<Draft>($state.snapshot(initial) as Draft);
	let headers = $state<[string, string][]>(
		// svelte-ignore state_referenced_locally
		initial.config.kind === 'webhook' ? Object.entries(initial.config.headers) : []
	);
	type NtfyAuth = 'none' | 'token' | 'login';
	let ntfyAuth = $state<NtfyAuth>(
		// svelte-ignore state_referenced_locally
		initial.config.kind !== 'ntfy'
			? 'none'
			: initial.config.token
				? 'token'
				: initial.config.username
					? 'login'
					: 'none'
	);
	let busy = $state(false);
	let testResult = $state<{ ok: boolean; text: string } | null>(null);
	let dialog: HTMLDialogElement;

	$effect(() => {
		dialog.showModal();
	});

	const triggers: { value: Trigger; label: string; hint: string }[] = [
		{ value: 'downloaded', label: 'Downloaded', hint: 'Chapters landed in the library' },
		{ value: 'failed', label: 'Failed', hint: 'A chapter gave up after its retries' },
		{ value: 'found', label: 'New chapters', hint: 'A source published new chapters' }
	];

	function input(): IntegrationInput {
		const config = $state.snapshot(draft.config) as Config;
		if (config.kind === 'webhook') {
			config.headers = Object.fromEntries(headers.filter(([k]) => k.trim()));
		}
		// Empty optional fields mean "every library".
		if (config.kind === 'komga') config.library_id = config.library_id?.trim() || null;
		if (config.kind === 'kavita') config.library_id = Number(config.library_id) || null;
		if (config.kind === 'ntfy') {
			config.topic = config.topic.trim();
			config.token = ntfyAuth === 'token' ? config.token?.trim() || null : null;
			config.username = ntfyAuth === 'login' ? config.username?.trim() || null : null;
			config.password = ntfyAuth === 'login' ? config.password || null : null;
			config.click_url = config.click_url?.trim() || null;
		}
		if (config.kind === 'apprise') {
			config.urls = config.urls?.trim() || null;
			config.key = config.urls ? null : config.key?.trim() || null;
			config.tags = config.urls ? null : config.tags?.trim() || null;
		}
		return { name: draft.name, enabled: draft.enabled, events: draft.events, config };
	}

	async function test() {
		busy = true;
		testResult = null;
		try {
			await api.testConfig(input().config, draft.id);
			testResult = {
				ok: true,
				text:
					draft.config.kind === 'komga' || draft.config.kind === 'kavita'
						? 'Connected'
						: 'Test sent'
			};
		} catch (e) {
			testResult = { ok: false, text: errorMessage(e) };
		} finally {
			busy = false;
		}
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		try {
			if (draft.id === undefined) await api.createIntegration(input());
			else await api.replaceIntegration(draft.id, input());
			toasts.show(`Saved ${draft.name}`);
			onsaved();
		} catch (e) {
			toasts.error(e);
		} finally {
			busy = false;
		}
	}

	async function remove() {
		if (draft.id === undefined || !confirm(`Delete ${draft.name}?`)) return;
		try {
			await api.deleteIntegration(draft.id);
			toasts.show(`Deleted ${draft.name}`);
			onsaved();
		} catch (e) {
			toasts.error(e);
		}
	}

	function toggleTrigger(t: Trigger, on: boolean) {
		draft.events = on ? [...draft.events, t] : draft.events.filter((x) => x !== t);
	}

	const library = $derived(draft.config.kind === 'komga' || draft.config.kind === 'kavita');
</script>

<dialog
	bind:this={dialog}
	{onclose}
	class="m-auto w-[min(640px,calc(100vw-24px))] rounded-2xl border border-line-strong bg-surface p-0 text-ink backdrop:bg-rail/80"
	aria-labelledby="editor-title"
>
	<form class="flex max-h-[85dvh] flex-col" onsubmit={save}>
		<header class="flex items-center justify-between gap-3 border-b border-line px-6 py-4">
			<h2 id="editor-title" class="m-0 font-display text-xl font-bold">
				{draft.id === undefined ? 'New' : 'Edit'}
				{kindLabel[draft.config.kind]}
			</h2>
			<label class="flex items-center gap-2 text-sm">
				<input type="checkbox" role="switch" bind:checked={draft.enabled} /> On
			</label>
		</header>

		<div class="flex flex-col gap-4 overflow-y-auto px-6 py-5">
			{#if hint}<p class="m-0 rounded-lg bg-raised px-3 py-2 text-sm text-ink-2">{hint}</p>{/if}

			<label class="flex flex-col gap-1.5 text-sm font-medium">
				Name
				<input type="text" required bind:value={draft.name} class="field font-normal" />
			</label>

			<label class="flex flex-col gap-1.5 text-sm font-medium">
				URL
				<input
					type="url"
					required
					bind:value={draft.config.url}
					class="field font-mono text-sm font-normal"
				/>
			</label>

			{#if draft.config.kind === 'komga' || draft.config.kind === 'kavita'}
				<p class="m-0 text-sm text-muted">
					kaze writes into its library folder and asks {kindLabel[draft.config.kind]} to rescan. Mount
					the same folder into {kindLabel[draft.config.kind]} (read-only is fine) and add it as a library
					there.
				</p>
				<label class="flex flex-col gap-1.5 text-sm font-medium">
					API key
					<input
						type="password"
						required
						autocomplete="off"
						bind:value={draft.config.api_key}
						class="field font-mono text-sm font-normal"
					/>
					<span class="font-normal text-muted">
						{draft.config.kind === 'komga'
							? 'Komga → Account settings → API keys.'
							: 'Kavita → User settings → API key.'}
					</span>
				</label>
				<label class="flex flex-col gap-1.5 text-sm font-medium">
					Library ID <span class="font-normal text-muted">(optional)</span>
					<input
						type={draft.config.kind === 'kavita' ? 'number' : 'text'}
						bind:value={draft.config.library_id}
						placeholder="Every library"
						class="field w-56 font-mono text-sm font-normal"
					/>
				</label>
			{:else if draft.config.kind === 'ntfy'}
				<label class="flex flex-col gap-1.5 text-sm font-medium">
					Topic
					<input
						type="text"
						required
						bind:value={draft.config.topic}
						placeholder="my-manga"
						class="field w-64 font-mono text-sm font-normal"
					/>
					<span class="font-normal text-muted"
						>On ntfy.sh anyone who knows the topic can read it: pick something hard to guess.</span
					>
				</label>
				<div class="flex flex-col gap-1.5 text-sm font-medium">
					Access
					<div class="self-start">
						<Segmented
							label="Access"
							bind:value={ntfyAuth}
							options={[
								{ value: 'none', label: 'None' },
								{ value: 'token', label: 'Token' },
								{ value: 'login', label: 'User and password' }
							]}
						/>
					</div>
				</div>
				{#if ntfyAuth === 'token'}
					<label class="flex flex-col gap-1.5 text-sm font-medium">
						Access token
						<input
							type="password"
							required
							autocomplete="off"
							bind:value={draft.config.token}
							placeholder="tk_…"
							class="field font-mono text-sm font-normal"
						/>
					</label>
				{:else if ntfyAuth === 'login'}
					<div class="flex flex-wrap gap-3">
						<label class="flex min-w-0 flex-1 flex-col gap-1.5 text-sm font-medium">
							User
							<input
								type="text"
								required
								autocomplete="off"
								bind:value={draft.config.username}
								class="field font-normal"
							/>
						</label>
						<label class="flex min-w-0 flex-1 flex-col gap-1.5 text-sm font-medium">
							Password
							<input
								type="password"
								required
								autocomplete="off"
								bind:value={draft.config.password}
								class="field font-normal"
							/>
						</label>
					</div>
				{/if}
				<label class="flex flex-col gap-1.5 text-sm font-medium">
					kaze address <span class="font-normal text-muted">(optional)</span>
					<input
						type="url"
						bind:value={draft.config.click_url}
						placeholder="https://kaze.example.com"
						class="field font-mono text-sm font-normal"
					/>
					<span class="font-normal text-muted"
						>Tapping a notification opens the series in kaze. Leave empty for no link.</span
					>
				</label>
			{:else if draft.config.kind === 'gotify'}
				<label class="flex flex-col gap-1.5 text-sm font-medium">
					Application token
					<input
						type="password"
						required
						autocomplete="off"
						bind:value={draft.config.token}
						class="field font-mono text-sm font-normal"
					/>
					<span class="font-normal text-muted">Gotify → Apps → Create application.</span>
				</label>
			{:else if draft.config.kind === 'apprise'}
				<label class="flex flex-col gap-1.5 text-sm font-medium">
					Notification URLs
					<input
						type="text"
						bind:value={draft.config.urls}
						placeholder="ntfy://ntfy.sh/my-topic"
						class="field font-mono text-sm font-normal"
					/>
					<span class="font-normal text-muted"
						>Apprise URLs, separated by spaces or commas, such as
						<code class="font-mono">ntfy://host/topic</code> or
						<code class="font-mono">tgram://token/chat</code>. Leave empty to use a key saved on
						apprise-api instead.</span
					>
				</label>
				{#if !draft.config.urls?.trim()}
					<label class="flex flex-col gap-1.5 text-sm font-medium">
						Configuration key
						<input
							type="text"
							required
							bind:value={draft.config.key}
							class="field w-56 font-mono text-sm font-normal"
						/>
						<span class="font-normal text-muted"
							>Posts to <code class="font-mono">{'{URL}'}/notify/{draft.config.key || '…'}</code> on your
							apprise-api server.</span
						>
					</label>
					<label class="flex flex-col gap-1.5 text-sm font-medium">
						Tags <span class="font-normal text-muted">(optional)</span>
						<input type="text" bind:value={draft.config.tags} class="field w-56 font-normal" />
					</label>
				{/if}
			{:else if draft.config.kind === 'webhook'}
				<label class="flex items-center gap-3 text-sm font-medium">
					Method
					<select bind:value={draft.config.method} class="field-sm w-auto font-sans">
						<option value="POST">POST</option>
						<option value="PUT">PUT</option>
					</select>
				</label>
				<fieldset class="m-0 flex flex-col gap-2 border-0 p-0">
					<legend class="mb-1.5 text-sm font-medium">Headers</legend>
					{#each headers as header, i (i)}
						<div class="flex gap-2">
							<input
								aria-label="Header name"
								placeholder="Authorization"
								bind:value={header[0]}
								class="field-sm min-w-0 flex-1"
							/>
							<input
								aria-label="Header value"
								placeholder="Bearer …"
								bind:value={header[1]}
								class="field-sm min-w-0 flex-[2]"
							/>
							<button
								type="button"
								class="icon-btn"
								aria-label="Remove header"
								onclick={() => headers.splice(i, 1)}>✕</button
							>
						</div>
					{/each}
					<button
						type="button"
						class="btn-ghost h-9 self-start text-sm"
						onclick={() => headers.push(['', ''])}>Add header</button
					>
				</fieldset>
				<label class="flex flex-col gap-1.5 text-sm font-medium">
					Body
					<textarea
						rows="8"
						required
						bind:value={draft.config.body}
						spellcheck="false"
						class="rounded-[10px] border border-line-strong bg-ground p-3 font-mono text-[13px] font-normal text-ink"
					></textarea>
					<span class="font-normal text-muted">
						A <a href="https://docs.rs/minijinja" target="_blank" rel="noreferrer">minijinja</a>
						template:
						<code class="font-mono">title</code>, <code class="font-mono">message</code>,
						<code class="font-mono">event</code>, <code class="font-mono">count</code>,
						<code class="font-mono">series</code>. Use <code class="font-mono">| tojson</code> for text
						inside JSON.
					</span>
				</label>
			{/if}

			{#if !library}
				<fieldset class="m-0 flex flex-col gap-2 border-0 p-0">
					<legend class="mb-1.5 text-sm font-medium">Send when</legend>
					{#each triggers as t (t.value)}
						<label class="flex items-start gap-2.5 text-sm">
							<input
								type="checkbox"
								class="mt-1"
								checked={draft.events.includes(t.value)}
								onchange={(e) => toggleTrigger(t.value, e.currentTarget.checked)}
							/>
							<span
								><span class="font-medium">{t.label}</span>
								<span class="text-muted">· {t.hint}</span></span
							>
						</label>
					{/each}
				</fieldset>
			{:else}
				<p class="m-0 text-sm text-muted">
					Scans once downloads settle down, not after every chapter.
				</p>
			{/if}

			{#if testResult}
				<p
					class="m-0 rounded-lg px-3 py-2 text-sm {testResult.ok
						? 'bg-ok-wash text-ok'
						: 'bg-bad-wash text-bad'}"
					role="status"
				>
					{testResult.text}
				</p>
			{/if}
		</div>

		<footer class="flex flex-wrap items-center gap-2.5 border-t border-line px-6 py-4">
			{#if draft.id !== undefined}
				<button type="button" class="btn-danger" onclick={remove}>Delete</button>
			{/if}
			<span class="flex-1"></span>
			<button type="button" class="btn-ghost" disabled={busy} onclick={test}>
				{library ? 'Test connection' : 'Send test'}
			</button>
			<button type="button" class="btn-ghost" onclick={() => dialog.close()}>Cancel</button>
			<button type="submit" class="btn-primary" disabled={busy}>Save</button>
		</footer>
	</form>
</dialog>
