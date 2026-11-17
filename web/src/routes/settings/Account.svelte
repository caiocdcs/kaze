<script lang="ts">
	import { api } from '#lib/api/index.ts';
	import { toasts } from '#lib/toast.svelte.ts';

	let current = $state('');
	let next = $state('');
	let busy = $state(false);

	async function change(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		try {
			await api.changePassword({ current, new: next });
			current = next = '';
			toasts.show('Password changed; other browsers were signed out');
		} catch (e) {
			toasts.error(e);
		} finally {
			busy = false;
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

<section class="flex max-w-3xl flex-col gap-4 pt-8 pb-6" aria-labelledby="account-heading">
	<h2 id="account-heading" class="section-title text-lg">Account</h2>
	<form class="flex flex-wrap items-end gap-3" onsubmit={change}>
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
		<button type="submit" class="btn-ghost" disabled={busy || !current || next.length < 8}
			>Change password</button
		>
	</form>
	<div>
		<button type="button" class="btn-ghost" onclick={signOut}>Sign out</button>
	</div>
</section>
