<script lang="ts">
	// The home's first block: what to watch now, in words on the left (an eyebrow, the title
	// large in Fraunces, a line of facts, the overview at a reading measure, the actions), its
	// still framed at 16:9 on the right on a wide screen, under it on a phone. Not a full-bleed
	// picture under the text: the words stay on the ground, legible in both themes.
	import type { Snippet } from 'svelte';
	import Poster from '#lib/components/Poster.svelte';

	interface Props {
		eyebrow: string;
		title: string;
		/** "Season 2 · Episode 4 · 55 min" */
		meta: readonly (string | null | undefined)[];
		overview?: string | null;
		art: string | null | undefined;
		/** Between the facts and the overview: what is left to watch. */
		progress?: Snippet;
		actions: Snippet;
		/** Under the actions: a line about them (the languages a play will use). */
		footer?: Snippet;
	}
	let { eyebrow, title, meta, overview, art, progress, actions, footer }: Props = $props();
	const id = $props.id();
	const facts = $derived(meta.filter((m): m is string => !!m));
</script>

<section class="hero" aria-labelledby="{id}-title">
	<div class="words">
		<p class="eyebrow">{eyebrow}</p>
		<h2 id="{id}-title" class="title">{title}</h2>
		{#if facts.length}<p class="facts">{facts.join(' · ')}</p>{/if}
		{#if progress}{@render progress()}{/if}
		{#if overview}<p class="overview">{overview}</p>{/if}
		<div class="actions">{@render actions()}</div>
		{#if footer}{@render footer()}{/if}
	</div>
	<div class="still">
		<Poster src={art} {title} shape="still" eager />
	</div>
</section>

<style>
	.hero {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(min(26rem, 100%), 1fr));
		gap: var(--s-5) var(--s-6);
		align-items: center;
		padding-block: var(--s-5) var(--s-6);
	}
	.words {
		display: grid;
		gap: var(--s-3);
		min-width: 0;
		align-content: start;
	}
	.title {
		font-family: var(--font-display);
		font-weight: 500;
		font-size: clamp(2.49rem, 2.03rem + 2.29vw, 4.74rem);
		line-height: 1.05;
		letter-spacing: -0.02em;
		overflow-wrap: anywhere;
	}
	.facts {
		margin: 0;
		font: var(--t-secondary);
		font-variant-numeric: tabular-nums;
		color: var(--ink-muted);
	}
	.overview {
		margin: 0;
		max-width: 38rem;
		color: var(--ink-muted);
		text-wrap: pretty;
	}
	.actions :global(.btn) {
		min-height: var(--control-h);
	}
	.still {
		min-width: 0;
	}
</style>
