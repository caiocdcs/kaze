<script lang="ts">
	import { ApiError, api, forgetLegacyKey, type AuthStatus } from '#lib/api/index.ts';
	import { auth } from '#lib/auth.svelte.ts';
	import { errorMessage } from '#lib/format.ts';

	let status = $state<AuthStatus | null>(null);
	let username = $state('');
	let password = $state('');
	/** Asked for when creating the account needs the API key. */
	let key = $state<string | null>(null);
	let error = $state<string | null>(null);
	let busy = $state(false);

	// The OIDC callback comes back here with what went wrong.
	$effect(() => {
		const params = new URLSearchParams(location.search);
		const failed = params.get('oidc_error');
		if (failed === null) return;
		error = failed;
		params.delete('oidc_error');
		const rest = params.toString();
		history.replaceState(history.state, '', location.pathname + (rest ? `?${rest}` : ''));
	});

	// On load, and again whenever a request is turned away (signed out
	// elsewhere, session expired).
	$effect(() => {
		void auth.needed;
		api
			.authStatus()
			.then((s) => {
				status = s;
				// Signed in, however: the API key an older UI kept is no longer needed here.
				if (s.signed_in) forgetLegacyKey();
			})
			.catch(() => {});
	});

	const setup = $derived(status !== null && !status.account);
	const open = $derived(status !== null && (!status.account || !status.signed_in || auth.needed));

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		error = null;
		try {
			const credentials = { username: username.trim(), password };
			if (setup) {
				await api.setup(credentials, key?.trim());
			} else {
				await api.login(credentials);
			}
			// Every open request and the event stream start over signed in.
			location.reload();
		} catch (e) {
			if (setup && e instanceof ApiError && e.status === 401) key ??= '';
			error = errorMessage(e);
			busy = false;
		}
	}
</script>

{#if open}
	<div class="fixed inset-0 z-40 flex items-center justify-center bg-rail/80 p-4 backdrop-blur-sm">
		<form
			class="card flex w-full max-w-md flex-col gap-4 p-6"
			onsubmit={submit}
			aria-labelledby="login-title"
		>
			<h2 id="login-title" class="section-title">{setup ? 'Create your account' : 'Sign in'}</h2>
			{#if setup}
				<p class="m-0 text-sm text-ink-2">
					kaze has no account yet. Pick a username and a password of at least 8 characters; you'll
					sign in with them from now on.
				</p>
			{/if}
			<label class="flex flex-col gap-1.5 text-sm">
				Username
				<!-- svelte-ignore a11y_autofocus -->
				<input
					type="text"
					bind:value={username}
					autofocus
					autocomplete="username"
					required
					class="field"
				/>
			</label>
			<label class="flex flex-col gap-1.5 text-sm">
				Password
				<input
					type="password"
					bind:value={password}
					autocomplete={setup ? 'new-password' : 'current-password'}
					minlength={setup ? 8 : undefined}
					required
					class="field"
				/>
			</label>
			{#if setup && key !== null}
				<label class="flex flex-col gap-1.5 text-sm">
					API key
					<input type="password" bind:value={key} autocomplete="off" class="field font-mono" />
					<span class="text-[13px] text-muted">
						Under Settings → General on a browser that has it, or <code class="font-mono"
							>api_key</code
						> in kaze.toml.
					</span>
				</label>
			{/if}
			{#if error}<p class="m-0 text-sm text-bad">{error}</p>{/if}
			<button type="submit" class="btn-primary" disabled={busy || !username.trim() || !password}>
				{setup ? 'Create account' : 'Sign in'}
			</button>
			{#if !setup && status?.oidc}
				<a class="btn-ghost" href={api.oidcLoginUrl()}>Sign in with {status.oidc}</a>
			{/if}
		</form>
	</div>
{/if}
