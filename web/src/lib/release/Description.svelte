<script lang="ts">
	// A release's notes as the tracker wrote them, in the format it declares (BBCode, HTML made
	// safe, plain text), in its own language (`lang`), folded to a few screens until « Read all ».
	import type { DescriptionFormat } from '@iris/api/client';
	import Icon from '#lib/components/Icon.svelte';
	import BBCode from './BBCode.svelte';
	import { sanitizeHtml } from './html.ts';
	import { textLang } from './language.ts';

	let { source, format = 'bbcode' }: { source: string; format?: DescriptionFormat } = $props();
	const id = $props.id();
	const html = $derived(format === 'html' ? sanitizeHtml(source) : '');
	const lang = $derived(textLang(source.replace(/<[^>]*>|\[[^\]]*\]/g, ' ')));
	let open = $state(false);
	let box = $state<HTMLElement>();
	let overflows = $state(false);

	$effect(() => {
		if (!box) return;
		const el = box;
		const measure = () => (overflows = el.scrollHeight > el.clientHeight + 1);
		const ro = new ResizeObserver(measure);
		ro.observe(el);
		return () => ro.disconnect();
	});
</script>

<div class="notes" class:open id="{id}-notes" {lang} bind:this={box}>
	{#if format === 'html'}
		<!-- sanitized by DOMPurify with an allow-list (html.ts) -->
		<div class="html">{@html html}</div>
	{:else if format === 'plain'}
		<p class="plain">{source}</p>
	{:else}
		<BBCode {source} />
	{/if}
</div>
{#if overflows || open}
	<button class="link-btn read" aria-expanded={open} aria-controls="{id}-notes" onclick={() => (open = !open)}>
		<Icon name={open ? 'chevron-up' : 'chevron-down'} size={16} />{open ? 'Show less' : 'Read all'}
	</button>
{/if}

<style>
	.notes {
		max-height: 24rem;
		overflow: hidden;
		overflow-wrap: anywhere;
		max-width: var(--measure);
	}
	.notes.open {
		max-height: none;
	}
	.plain {
		margin: 0;
		white-space: pre-wrap;
	}
	.html :global(:is(h1, h2, h3, h4, h5, h6)) {
		font: var(--t-group);
		margin: var(--s-3) 0 var(--s-1);
	}
	.html :global(p) {
		margin: var(--s-1) 0;
	}
	.html :global(img) {
		display: inline-block;
		max-width: 100%;
		max-height: 10rem;
		height: auto;
		border-radius: var(--radius-s);
	}
	.html :global(table) {
		border-collapse: collapse;
		font: var(--t-secondary);
		margin: var(--s-2) 0;
	}
	.html :global(:is(td, th)) {
		border: 1px solid var(--line);
		padding: var(--s-1) var(--s-2);
		text-align: left;
	}
	.read {
		display: inline-flex;
		align-items: center;
		gap: var(--s-1);
		min-height: var(--control-h);
	}
</style>
