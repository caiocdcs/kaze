<script lang="ts">
	import { api } from '#lib/api/index.ts';

	let {
		id,
		title,
		size = 'md',
		class: klass = ''
	}: { id?: number; title: string; size?: 'xs' | 'sm' | 'md' | 'lg'; class?: string } = $props();

	const tints = [
		'#3A2723',
		'#1F3140',
		'#2C2B45',
		'#25382F',
		'#3E2536',
		'#1E3936',
		'#3B3222',
		'#2A2F38'
	];
	const tint = $derived(
		tints[[...title].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7) % tints.length]
	);
	const font = { xs: 'text-[11px]', sm: 'text-[12px]', md: 'text-[22px]', lg: 'text-[30px]' };

	let failed = $state(false);
	$effect(() => {
		void id;
		failed = false;
	});
</script>

<!-- No cover yet? A tinted card with the title set vertically, as on the canvas. -->
<div
	class="relative flex aspect-[2/3] items-center justify-center overflow-hidden shadow-[inset_0_0_0_1px_rgba(255,255,255,0.06)] {klass}"
	style:background={tint}
>
	<span
		class="max-h-[86%] overflow-hidden font-display font-bold tracking-[0.08em] text-paper [writing-mode:vertical-rl] {font[
			size
		]}"
		aria-hidden={id !== undefined && !failed}>{title}</span
	>
	{#if id !== undefined && !failed}
		<img
			src={api.coverUrl(id)}
			alt=""
			loading="lazy"
			class="absolute inset-0 size-full object-cover"
			onerror={() => (failed = true)}
		/>
	{/if}
</div>
