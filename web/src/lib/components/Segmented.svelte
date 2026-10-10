<script lang="ts" generics="T extends string">
	let {
		options,
		value = $bindable(),
		label
	}: { options: { value: T; label: string }[]; value: T; label: string } = $props();
</script>

<div
	role="group"
	aria-label={label}
	class="flex gap-1 rounded-[10px] border border-line-strong bg-surface p-1"
>
	{#each options as opt (opt.value)}
		<button
			type="button"
			aria-pressed={value === opt.value}
			onclick={() => (value = opt.value)}
			class="h-[34px] cursor-pointer rounded-[7px] border-0 px-3 text-sm whitespace-nowrap {value ===
			opt.value
				? 'bg-line-strong text-ink'
				: 'bg-transparent text-muted hover:text-ink'}"
		>
			{opt.label}
		</button>
	{/each}
</div>
