<script lang="ts">
	import { page } from '$app/state';
	import { live } from '#lib/live.svelte.ts';
	import { jobLabel, num } from '#lib/format.ts';
	import type { IconName } from '#lib/icons.ts';
	import Icon from './Icon.svelte';
	import ProgressBar from './ProgressBar.svelte';

	type Item = { href: string; label: string; icon: IconName; tab?: boolean };
	const items: Item[] = [
		{ href: '/', label: 'Library', icon: 'library', tab: true },
		{ href: '/add', label: 'Add series', icon: 'add' },
		{ href: '/wanted', label: 'Wanted', icon: 'wanted', tab: true },
		{ href: '/activity', label: 'Activity', icon: 'activity', tab: true },
		{ href: '/settings', label: 'Settings', icon: 'settings', tab: true }
	];

	const active = (href: string) =>
		href === '/'
			? page.url.pathname === '/' || page.url.pathname.startsWith('/series')
			: page.url.pathname.startsWith(href);

	const current = $derived(live.running[0]);
	const currentProgress = $derived(current ? live.progress.get(current.id) : undefined);
</script>

<!-- Desktop: a side rail. -->
<nav
	aria-label="Main"
	class="sticky top-0 hidden h-dvh w-[232px] shrink-0 flex-col gap-1 border-r border-line bg-rail px-3.5 py-6 md:flex"
>
	<a href="/" class="flex items-baseline gap-2.5 px-2.5 pb-[22px] text-ink no-underline">
		<span class="font-display text-[30px] leading-none font-bold text-seal">風</span>
		<span class="font-display text-[21px] font-bold tracking-[0.06em]">kaze</span>
	</a>
	{#each items as item (item.href)}
		<a
			href={item.href}
			aria-current={active(item.href) ? 'page' : undefined}
			class="flex min-h-11 items-center gap-3 rounded-[9px] px-3 no-underline {active(item.href)
				? 'bg-raised font-semibold text-seal-soft'
				: 'text-ink-2 hover:bg-surface hover:text-ink'}"
		>
			<Icon name={item.icon} />
			<span class="flex-1">{item.label}</span>
			{#if item.href === '/wanted' && live.wanted > 0}
				<span class="rounded-full bg-wanted-wash px-[7px] py-px font-mono text-xs text-wanted"
					>{num(live.wanted)}</span
				>
			{/if}
		</a>
	{/each}

	<a
		href="/activity"
		class="mt-auto flex flex-col gap-2 rounded-[10px] border border-line bg-surface-2 p-3.5 text-ink no-underline hover:text-ink"
	>
		<span class="flex justify-between text-xs text-muted">
			<span>{current ? 'Downloading' : live.connected ? 'Idle' : 'Offline'}</span>
			<span class="font-mono">
				{live.running.length} · {num(live.queued.length)} queued
			</span>
		</span>
		{#if current}
			<span class="truncate text-[13px] font-medium">{jobLabel(current)}</span>
			<ProgressBar
				value={currentProgress ? currentProgress.done / Math.max(1, currentProgress.total) : 0}
			/>
		{:else}
			<span class="flex items-center gap-2 text-[13px] text-muted">
				<span class="size-2 rounded-full {live.connected ? 'bg-ok' : 'bg-bad'}" aria-hidden="true"
				></span>
				{live.connected ? 'Nothing running' : 'Reconnecting…'}
			</span>
		{/if}
	</a>
</nav>

<!-- Phone: a bottom tab bar, as on the Phone board. -->
<nav
	aria-label="Main"
	class="fixed inset-x-0 bottom-0 z-20 grid grid-cols-4 border-t border-line bg-rail px-2 pt-1.5 pb-[max(18px,env(safe-area-inset-bottom))] md:hidden"
>
	{#each items.filter((i) => i.tab) as item (item.href)}
		<a
			href={item.href}
			aria-current={active(item.href) ? 'page' : undefined}
			class="relative flex min-h-[52px] flex-col items-center justify-center gap-0.5 text-xs no-underline {active(
				item.href
			)
				? 'font-semibold text-seal-soft'
				: 'text-muted'}"
		>
			<Icon name={item.icon} size={20} />
			<span>{item.label}</span>
			{#if item.href === '/wanted' && live.wanted > 0}
				<span
					class="absolute top-1 left-[calc(50%+6px)] rounded-full bg-wanted px-1.5 font-mono text-[10px] text-wanted-ink"
					>{live.wanted}</span
				>
			{/if}
		</a>
	{/each}
</nav>
