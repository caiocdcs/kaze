<script lang="ts" module>
	export type ChipTone = 'seal' | 'wanted' | 'disk' | 'bad' | 'off' | 'ok';
	const tones: Record<ChipTone, [string, string]> = {
		seal: ['var(--color-seal-soft)', 'var(--color-seal-chip)'],
		wanted: ['var(--color-wanted)', 'var(--color-wanted-wash)'],
		disk: ['var(--color-disk)', 'var(--color-disk-wash)'],
		bad: ['var(--color-bad)', 'var(--color-bad-wash)'],
		off: ['var(--color-muted)', 'var(--color-raised)'],
		ok: ['var(--color-ok)', 'var(--color-ok-wash)']
	};
</script>

<script lang="ts">
	let { tone, label, title }: { tone: ChipTone; label: string; title?: string } = $props();
	const [fg, bg] = $derived(tones[tone]);
</script>

<span
	class="inline-flex max-w-full items-center gap-[7px] rounded-full px-2.5 py-[3px] text-[13px] font-medium whitespace-nowrap"
	style:color={fg}
	style:background={bg}
	{title}
>
	<span aria-hidden="true" class="size-[7px] shrink-0 rounded-full" style:background={fg}></span>
	<span class="truncate">{label}</span>
</span>
