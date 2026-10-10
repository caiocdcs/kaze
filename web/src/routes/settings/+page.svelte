<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import General from './General.svelte';
	import Sources from './Sources.svelte';
	import System from './System.svelte';

	const tabs = [
		{ id: 'general', label: 'General' },
		{ id: 'sources', label: 'Sources' },
		{ id: 'system', label: 'System' }
	] as const;
	type Tab = (typeof tabs)[number]['id'];

	const tab = $derived<Tab>(
		tabs.find((t) => t.id === page.url.searchParams.get('tab'))?.id ?? 'general'
	);
</script>

<svelte:head><title>Settings · kaze</title></svelte:head>

<h1 class="page-title">Settings</h1>

<div
	role="tablist"
	aria-label="Settings sections"
	class="-mt-3 flex gap-1 overflow-x-auto border-b border-line"
>
	{#each tabs as t (t.id)}
		<button
			type="button"
			role="tab"
			aria-selected={tab === t.id}
			onclick={() => goto(`?tab=${t.id}`, { replace: true, reset: false })}
			class="-mb-px h-11 cursor-pointer border-0 bg-transparent px-4 whitespace-nowrap {tab === t.id
				? 'font-semibold text-ink shadow-[inset_0_-2px_0_var(--color-seal)]'
				: 'text-muted hover:text-ink'}">{t.label}</button
		>
	{/each}
</div>

<div role="tabpanel">
	{#if tab === 'general'}
		<General />
	{:else if tab === 'sources'}
		<Sources />
	{:else}
		<System />
	{/if}
</div>
