<script lang="ts">
	import { icons, type IconName } from '#lib/icons.ts';

	let {
		name,
		size = 18,
		stroke = 1.8,
		class: klass = ''
	}: { name: IconName; size?: number; stroke?: number; class?: string } = $props();
</script>

<svg
	width={size}
	height={size}
	viewBox="0 0 24 24"
	fill="none"
	stroke="currentColor"
	stroke-width={stroke}
	stroke-linecap="round"
	stroke-linejoin="round"
	aria-hidden="true"
	class="shrink-0 {klass}"><path d={icons[name]} /></svg
>
