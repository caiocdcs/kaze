<script lang="ts">
	import { toasts } from '#lib/toast.svelte.ts';
	import Icon from './Icon.svelte';
</script>

<div
	class="pointer-events-none fixed right-4 bottom-24 z-50 flex w-[min(380px,calc(100vw-32px))] flex-col gap-2 md:bottom-6"
	aria-live="polite"
>
	{#each toasts.list as t (t.id)}
		<div
			class="pointer-events-auto flex items-start gap-3 rounded-xl border px-4 py-3 text-sm shadow-lg {t.tone ===
			'error'
				? 'border-bad-line bg-bad-wash text-bad'
				: 'border-line-strong bg-surface text-ink'}"
			role={t.tone === 'error' ? 'alert' : 'status'}
		>
			<span class="flex-1">{t.message}</span>
			<button
				type="button"
				class="-m-1 cursor-pointer rounded p-1 text-current opacity-70 hover:opacity-100"
				aria-label="Dismiss"
				onclick={() => toasts.dismiss(t.id)}><Icon name="close" size={16} /></button
			>
		</div>
	{/each}
</div>
