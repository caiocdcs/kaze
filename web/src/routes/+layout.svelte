<script lang="ts">
	import '../app.css';
	import { onMount } from 'svelte';
	import ApiKeyPrompt from '#lib/components/ApiKeyPrompt.svelte';
	import Nav from '#lib/components/Nav.svelte';
	import Toasts from '#lib/components/Toasts.svelte';
	import { live } from '#lib/live.svelte.ts';
	import { toasts } from '#lib/toast.svelte.ts';

	let { children } = $props();

	onMount(() => live.start());
</script>

<svelte:window
	onerror={(e) => toasts.error(e instanceof ErrorEvent ? (e.error ?? e.message) : e)}
	onunhandledrejection={(e) => toasts.error(e.reason)}
/>

<div class="flex min-h-dvh">
	<Nav />
	<main
		class="flex min-w-0 flex-1 flex-col gap-7 px-4 pt-6 pb-28 sm:px-6 md:px-10 md:pt-8 md:pb-12"
	>
		{@render children()}
	</main>
</div>
<Toasts />
<ApiKeyPrompt />
