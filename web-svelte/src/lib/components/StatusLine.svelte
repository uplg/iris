<script lang="ts" module>
	/** What a title or a release is, said in words, its shape helping (never color alone):
	 * on disk (a filled check), moving (a spinner), to grab (a download arrow), a problem (a
	 * warning), or plain information. */
	export type Tone = 'ok' | 'busy' | 'available' | 'warn' | 'info';
</script>

<script lang="ts">
	import Icon, { type IconName } from './Icon.svelte';

	let { tone = 'info', text }: { tone?: Tone; text: string } = $props();
	const ICON: Record<Tone, IconName | null> = {
		ok: 'circle-check',
		busy: 'loader-circle',
		available: 'download',
		warn: 'triangle-alert',
		info: null
	};
</script>

<p class="status {tone}">
	{#if ICON[tone]}<Icon name={ICON[tone]!} size={16} busy={tone === 'busy'} />{/if}
	<span>{text}</span>
</p>

<style>
	.status {
		display: flex;
		align-items: center;
		gap: var(--s-1);
		margin: 0;
		font: var(--t-meta);
		font-variant-numeric: tabular-nums;
		color: var(--ink-muted);
	}
	.ok,
	.busy {
		color: var(--accent);
	}
	.warn {
		color: var(--warn-text);
	}
</style>
