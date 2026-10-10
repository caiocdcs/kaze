<script lang="ts">
	import { setApiKey } from '#lib/api/index.ts';
	import { auth } from '#lib/auth.svelte.ts';

	let key = $state('');

	function save(event: SubmitEvent) {
		event.preventDefault();
		setApiKey(key.trim() || null);
		// Every open request and the event stream need the key: start over.
		location.reload();
	}
</script>

{#if auth.needed}
	<div class="fixed inset-0 z-40 flex items-center justify-center bg-rail/80 p-4 backdrop-blur-sm">
		<form
			class="card flex w-full max-w-md flex-col gap-4 p-6"
			onsubmit={save}
			aria-labelledby="key-title"
		>
			<h2 id="key-title" class="section-title">API key needed</h2>
			<p class="m-0 text-sm text-ink-2">
				This kaze requires an API key. It's in <code class="font-mono">kaze.toml</code> as
				<code class="font-mono">api_key</code>, or in Settings on a browser that already has it.
				It's kept in this browser only.
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
