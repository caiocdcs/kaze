<script lang="ts">
	import { resolve } from '$app/paths';
	import { api, type Settings, type SettingsView } from '#lib/api/index.ts';
	import { errorMessage } from '#lib/format.ts';
	import { toasts } from '#lib/toast.svelte.ts';

	let view = $state<SettingsView | null>(null);
	let error = $state<string | null>(null);
	let saving = $state(false);

	let current = $state('');
	let next = $state('');
	let changingPassword = $state(false);

	let apiKey = $state('');
	let revealKey = $state(false);

	let sso = $state({
		on: false,
		issuer: '',
		client_id: '',
		client_secret: '',
		public_url: '',
		name: '',
		allowed: ''
	});

	$effect(() => {
		api
			.settings()
			.then(set)
			.catch((e) => (error = errorMessage(e)));
	});

	function set(v: SettingsView) {
		view = v;
		apiKey = v.settings.api_key ?? '';
		const o = v.settings.oidc;
		sso = {
			on: o !== null,
			issuer: o?.issuer ?? '',
			client_id: o?.client_id ?? '',
			// The server sends the saved secret masked, and keeps it when the mask comes back.
			client_secret: o?.client_secret ?? '',
			public_url: o?.public_url ?? location.origin,
			name: o?.name ?? '',
			allowed: (o?.allowed ?? []).join('\n')
		};
	}

	const locked = (k: keyof Settings) => view?.locked.includes(k) ?? false;
	const callback = $derived(
		`${sso.public_url.trim().replace(/\/$/, '')}${resolve('/').replace(/\/$/, '')}/api/v1/auth/oidc/callback`
	);

	async function save(changes: Parameters<typeof api.updateSettings>[0], done: string) {
		saving = true;
		try {
			set(await api.updateSettings(changes));
			toasts.show(done);
		} catch (e) {
			toasts.error(e);
		} finally {
			saving = false;
		}
	}

	function saveKey(event: SubmitEvent) {
		event.preventDefault();
		// Saved empty, not removed, or kaze would make a new key on restart.
		save({ api_key: apiKey.trim() }, 'API key saved');
	}

	function saveSso(event: SubmitEvent) {
		event.preventDefault();
		const oidc = sso.on
			? {
					issuer: sso.issuer.trim(),
					client_id: sso.client_id.trim(),
					client_secret: sso.client_secret,
					public_url: sso.public_url.trim(),
					name: sso.name.trim() || null,
					allowed: sso.allowed
						.split(/[\n,]/)
						.map((a) => a.trim())
						.filter(Boolean)
				}
			: null;
		save({ oidc }, sso.on ? 'Single sign-on saved' : 'Single sign-on turned off');
	}

	function generateKey() {
		const bytes = crypto.getRandomValues(new Uint8Array(18));
		apiKey = 'kz_' + [...bytes].map((b) => b.toString(16).padStart(2, '0')).join('');
		revealKey = true;
	}

	async function copy(text: string, what: string) {
		try {
			await navigator.clipboard.writeText(text);
			toasts.show(`${what} copied`);
		} catch {
			toasts.show('Copy failed; select it and copy it by hand', 'error');
		}
	}

	async function changePassword(event: SubmitEvent) {
		event.preventDefault();
		changingPassword = true;
		try {
			await api.changePassword({ current, new: next });
			current = next = '';
			toasts.show('Password changed; other browsers were signed out');
		} catch (e) {
			toasts.error(e);
		} finally {
			changingPassword = false;
		}
	}

	async function signOut() {
		try {
			await api.logout();
			location.reload();
		} catch (e) {
			toasts.error(e);
		}
	}
</script>

{#if error}
	<p class="card m-0 border-bad-line p-4 text-bad">Couldn't load settings: {error}</p>
{:else if view}
	<div class="flex max-w-3xl flex-col gap-8 pt-6">
		<section class="flex flex-col gap-4" aria-labelledby="account-heading">
			<div class="flex flex-wrap items-center justify-between gap-3">
				<h2 id="account-heading" class="section-title text-lg">Account</h2>
				<button type="button" class="btn-ghost" onclick={signOut}>Sign out</button>
			</div>
			<form class="flex flex-wrap items-end gap-3" onsubmit={changePassword}>
				<label class="flex flex-col gap-1.5">
					<span class="font-medium">Current password</span>
					<input
						type="password"
						bind:value={current}
						autocomplete="current-password"
						required
						class="field w-56"
					/>
				</label>
				<label class="flex flex-col gap-1.5">
					<span class="font-medium">New password</span>
					<input
						type="password"
						bind:value={next}
						autocomplete="new-password"
						minlength="8"
						required
						class="field w-56"
					/>
				</label>
				<button
					type="submit"
					class="btn-ghost"
					disabled={changingPassword || !current || next.length < 8}>Change password</button
				>
			</form>
			<span class="text-[13px] text-muted"
				>Changing the password signs out every other browser.</span
			>
		</section>

		<form class="flex flex-col gap-4" aria-labelledby="key-heading" onsubmit={saveKey}>
			<h2 id="key-heading" class="section-title flex items-center gap-2 text-lg">
				API key
				{#if locked('api_key')}<span class="lock">set in kaze.toml</span>{/if}
			</h2>
			<div class="flex flex-wrap gap-2">
				<input
					aria-labelledby="key-heading"
					type={revealKey ? 'text' : 'password'}
					bind:value={apiKey}
					disabled={locked('api_key')}
					placeholder="None: scripts can't use the API"
					autocomplete="off"
					class="field min-w-0 flex-[1_1_280px] font-mono text-sm"
				/>
				<button type="button" class="btn-ghost" onclick={() => (revealKey = !revealKey)}>
					{revealKey ? 'Hide' : 'Show'}
				</button>
				<button
					type="button"
					class="btn-ghost"
					onclick={() => copy(apiKey, 'API key')}
					disabled={!apiKey}>Copy</button
				>
				{#if !locked('api_key')}
					<button type="button" class="btn-ghost" onclick={generateKey}>Generate</button>
				{/if}
			</div>
			<span class="text-[13px] text-muted">
				For scripts and other apps: they send it as <code class="font-mono">X-Api-Key</code>.
				Browsers sign in with the account instead.
			</span>
			{#if !locked('api_key')}
				<div>
					<button
						type="submit"
						class="btn-primary"
						disabled={saving || apiKey.trim() === (view.settings.api_key ?? '')}>Save key</button
					>
				</div>
			{/if}
		</form>

		<form class="flex flex-col gap-4" aria-labelledby="sso-heading" onsubmit={saveSso}>
			<h2 id="sso-heading" class="section-title flex items-center gap-2 text-lg">
				Single sign-on
				{#if locked('oidc')}<span class="lock">set in kaze.toml</span>{/if}
			</h2>
			<label class="flex items-start gap-3">
				<input type="checkbox" bind:checked={sso.on} disabled={locked('oidc')} class="mt-1" />
				<span class="flex flex-col gap-0.5">
					<span class="font-medium">Sign in with an OpenID Connect provider</span>
					<span class="text-[13px] text-muted"
						>Authelia, Authentik, Pocket ID, Keycloak… The password keeps working, for when the
						provider is down.</span
					>
				</span>
			</label>
			{#if sso.on}
				<fieldset
					class="m-0 grid gap-x-6 gap-y-5 border-0 p-0 sm:grid-cols-2"
					disabled={locked('oidc')}
				>
					<label class="flex flex-col gap-1.5 sm:col-span-2">
						<span class="font-medium">Issuer</span>
						<input
							type="url"
							bind:value={sso.issuer}
							required
							placeholder="https://auth.example.com"
							class="field font-mono text-sm"
						/>
						<span class="text-[13px] text-muted"
							>Exactly as the provider names itself, trailing slash or not.</span
						>
					</label>
					<label class="flex flex-col gap-1.5">
						<span class="font-medium">Client ID</span>
						<input
							type="text"
							bind:value={sso.client_id}
							required
							autocomplete="off"
							class="field font-mono text-sm"
						/>
					</label>
					<label class="flex flex-col gap-1.5">
						<span class="font-medium">Client secret</span>
						<input
							type="password"
							bind:value={sso.client_secret}
							required
							autocomplete="off"
							class="field font-mono text-sm"
						/>
					</label>
					<label class="flex flex-col gap-1.5">
						<span class="font-medium">kaze's address</span>
						<input
							type="url"
							bind:value={sso.public_url}
							required
							class="field font-mono text-sm"
						/>
						<span class="text-[13px] text-muted">Where browsers reach kaze.</span>
					</label>
					<label class="flex flex-col gap-1.5">
						<span class="font-medium">Button name</span>
						<input type="text" bind:value={sso.name} placeholder="single sign-on" class="field" />
						<span class="text-[13px] text-muted">Shows as "Sign in with …".</span>
					</label>
					<label class="flex flex-col gap-1.5 sm:col-span-2">
						<span class="font-medium">Who may sign in</span>
						<textarea
							bind:value={sso.allowed}
							rows="3"
							placeholder="me@example.com"
							class="field h-auto py-2.5 font-mono text-sm"></textarea>
						<span class="text-[13px] text-muted"
							>Emails or subject IDs, one per line. Empty lets in anyone the provider signs in for
							this client.</span
						>
					</label>
					<div class="flex flex-col gap-1.5 sm:col-span-2">
						<span class="font-medium">Callback URL to register with the provider</span>
						<div class="flex flex-wrap gap-2">
							<code
								class="field flex min-w-0 flex-[1_1_280px] items-center font-mono text-sm break-all"
								>{callback}</code
							>
							<button type="button" class="btn-ghost" onclick={() => copy(callback, 'Callback URL')}
								>Copy</button
							>
						</div>
					</div>
				</fieldset>
			{/if}
			{#if !locked('oidc')}
				<div>
					<button type="submit" class="btn-primary" disabled={saving}>Save single sign-on</button>
				</div>
			{/if}
		</form>
	</div>
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
