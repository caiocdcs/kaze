<script lang="ts">
	let {
		label,
		hint,
		value = $bindable(),
		known,
		id
	}: { label: string; hint: string; value: string[]; known: string[]; id: string } = $props();

	let draft = $state('');

	const has = (name: string) => value.some((v) => v.toLowerCase() === name.toLowerCase());

	function add() {
		const name = draft.trim();
		if (name && !has(name)) value = [...value, name];
		draft = '';
	}
</script>

<div class="flex flex-col gap-1.5 text-sm text-ink-2">
	<label for={id}>{label}</label>
	{#if value.length}
		<ul class="m-0 flex list-none flex-wrap gap-1.5 p-0">
			{#each value as name (name)}
				<li class="flex items-center gap-1 rounded-full bg-raised py-[3px] pr-1 pl-2.5 text-[13px]">
					{name}
					<button
						type="button"
						class="cursor-pointer border-0 bg-transparent px-1 text-muted hover:text-bad"
						aria-label="Remove {name}"
						onclick={() => (value = value.filter((v) => v !== name))}>×</button
					>
				</li>
			{/each}
		</ul>
	{/if}
	<input
		{id}
		type="text"
		list="{id}-known"
		placeholder={hint}
		bind:value={draft}
		onkeydown={(e) => {
			if (e.key === 'Enter') {
				e.preventDefault();
				add();
			}
		}}
		onchange={() => known.includes(draft) && add()}
		class="field-sm"
	/>
	<datalist id="{id}-known">
		{#each known.filter((k) => !has(k)) as name (name)}
			<option value={name}></option>
		{/each}
	</datalist>
</div>
