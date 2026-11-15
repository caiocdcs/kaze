<script lang="ts">
	import { claimApiKey, setApiKey } from '#lib/api/index.ts';
	import { auth } from '#lib/auth.svelte.ts';
	import { toasts } from '#lib/toast.svelte.ts';

	let key = $state('');
	/** The key kaze generated on its first start, shown here once. */
	let claimed = $state<string | null>(null);

	$effect(() => {
		if (!auth.needed) return;
		claimApiKey().then((k) => {
			if (!k) return;
			// Saved at once, so a reload can't lose it.
			setApiKey(k);
			claimed = k;
		});
	});

	async function copyClaimed() {
		if (!claimed) return;
		try {
			await navigator.clipboard.writeText(claimed);
			toasts.show('API key copied');
		} catch {
			toasts.show('Copy failed; select the key and copy it by hand', 'error');
		}
	}

	function save(event: SubmitEvent) {
		event.preventDefault();
		setApiKey(key.trim() || null);
		// Every open request and the event stream need the key: start over.
		location.reload();
	}
</script>

{#if auth.needed && claimed}
	<div class="fixed inset-0 z-40 flex items-center justify-center bg-rail/80 p-4 backdrop-blur-sm">
		<div class="card flex w-full max-w-md flex-col gap-4 p-6" aria-labelledby="claimed-title">
			<h2 id="claimed-title" class="section-title">Your API key</h2>
			<p class="m-0 text-sm text-ink-2">
				kaze made this key on its first start, and this browser now has it saved. Copy it for other
				apps and browsers: it's shown here only once, and later under Settings → General.
			</p>
			<code class="field font-mono text-sm break-all select-all">{claimed}</code>
			<div class="flex gap-2">
				<button type="button" class="btn-ghost" onclick={copyClaimed}>Copy</button>
				<button type="button" class="btn-primary" onclick={() => location.reload()}>Continue</button
				>
			</div>
		</div>
	</div>
{:else if auth.needed}
	<div class="fixed inset-0 z-40 flex items-center justify-center bg-rail/80 p-4 backdrop-blur-sm">
		<form
			class="card flex w-full max-w-md flex-col gap-4 p-6"
			onsubmit={save}
			aria-labelledby="key-title"
		>
			<h2 id="key-title" class="section-title">API key needed</h2>
			<p class="m-0 text-sm text-ink-2">
				This kaze requires an API key. It's under Settings → General on a browser that already has
				it, or in <code class="font-mono">kaze.toml</code> as
				<code class="font-mono">api_key</code> if you set it there. It's kept in this browser only.
			</p>
			<label class="flex flex-col gap-1.5 text-sm">
				API key
				<!-- svelte-ignore a11y_autofocus -->
				<input
					type="password"
					bind:value={key}
					autofocus
					autocomplete="off"
					class="field font-mono"
				/>
			</label>
			<button type="submit" class="btn-primary" disabled={!key.trim()}>Save and reload</button>
		</form>
	</div>
{/if}
