<script lang="ts">
	let {
		value,
		color = 'var(--color-seal)',
		height = 4,
		label
	}: { value: number; color?: string; height?: number; label?: string } = $props();
	const pct = $derived(Math.max(0, Math.min(100, Math.round(value * 100))));
</script>

<span
	class="block overflow-hidden rounded bg-track"
	style:height="{height}px"
	role={label ? 'progressbar' : undefined}
	aria-label={label}
	aria-valuenow={label ? pct : undefined}
	aria-valuemin={label ? 0 : undefined}
	aria-valuemax={label ? 100 : undefined}
>
	<span
		class="block h-full transition-[width] duration-300"
		style:width="{pct}%"
		style:background={color}
	></span>
</span>
