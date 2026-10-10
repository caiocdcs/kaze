<script lang="ts">
	import type { Monitor } from '#lib/api/types/Monitor.ts';

	let {
		value = $bindable(),
		name,
		legend,
		hints = {}
	}: {
		value: Monitor;
		name: string;
		legend: string;
		/** Per-option override of the description line. */
		hints?: Partial<Record<Monitor, string>>;
	} = $props();

	const options: { value: Monitor; label: string; hint: string }[] = [
		{ value: 'all', label: 'All chapters', hint: 'Past and future' },
		{ value: 'future', label: 'New chapters only', hint: 'From now on' },
		{ value: 'none', label: 'None', hint: 'Track, don’t download' }
	];
</script>

<fieldset class="m-0 flex flex-col gap-2 border-0 p-0">
	<legend class="mb-2 text-sm font-semibold">{legend}</legend>
	{#each options as opt (opt.value)}
		<label
			class="flex cursor-pointer items-start gap-2.5 rounded-[10px] border px-3 py-2.5 {value ===
			opt.value
				? 'border-seal-line bg-seal-wash'
				: 'border-line-strong hover:bg-raised'}"
		>
			<input type="radio" {name} value={opt.value} bind:group={value} class="mt-1" />
			<span>
				<span class="block font-semibold">{opt.label}</span>
				<span class="text-[13px] text-muted">{hints[opt.value] ?? opt.hint}</span>
			</span>
		</label>
	{/each}
</fieldset>
