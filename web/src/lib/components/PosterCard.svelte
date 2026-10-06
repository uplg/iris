<script lang="ts">
	// A title in a row or a grid: its artwork, its name (the one link, opened from anywhere on
	// the card), one line of facts and its state in words. A still (16:9) for what is being
	// watched, with what is left.
	import Meter from '#lib/components/Meter.svelte';
	import type { Snippet } from 'svelte';
	import Poster from './Poster.svelte';
	import StatusLine, { type Tone } from './StatusLine.svelte';

	interface Props {
		href: string;
		title: string;
		art: string | null | undefined;
		shape?: 'poster' | 'still';
		/** "Series · 2022", "S2:E4 · The You You Are" */
		meta?: string;
		status?: { tone: Tone; text: string };
		/** Watched so far, 0 to 1: a bar, its words said in `meta`/`status`. */
		progress?: number;
		/** A menu beside the name (remove from the row…). */
		actions?: Snippet;
	}
	let { href, title, art, shape = 'poster', meta, status, progress, actions }: Props = $props();
</script>

<li class="card whole-card {shape}">
	<Poster src={art} {title} {shape} />
	{#if progress !== undefined}
		<Meter share={progress} />
	{/if}
	<div class="text">
		<div class="lines">
			<a class="name card-link" {href}>{title}</a>
			{#if meta}<span class="meta">{meta}</span>{/if}
			{#if status}<StatusLine tone={status.tone} text={status.text} />{/if}
		</div>
		{#if actions}<div class="actions">{@render actions()}</div>{/if}
	</div>
</li>

<style>
	/* one column no wider than the card: a title that is one long word (a release name)
	   would otherwise widen it to the word, the art with it */
	.card {
		display: grid;
		grid-template-columns: minmax(0, 1fr);
		gap: var(--s-2);
		align-content: start;
		min-width: 0;
	}
	.poster {
		flex: 0 0 var(--poster-w);
		width: var(--poster-w);
	}
	.still {
		flex: 0 0 var(--still-w);
		width: var(--still-w);
	}
	:global(.poster-grid) > .card {
		width: auto;
	}
	.text {
		display: flex;
		gap: var(--s-2);
		align-items: flex-start;
	}
	.actions {
		display: flex;
		flex: none;
		gap: var(--s-2);
	}
	.lines {
		flex: 1;
		min-width: 0;
		display: grid;
		gap: var(--s-half);
	}
	.name {
		font: var(--t-body);
		font-weight: 600;
		color: var(--ink);
		text-decoration: none;
		overflow-wrap: anywhere;
	}
	.name:hover {
		text-decoration: underline;
		text-underline-offset: 2px;
	}
	.meta {
		font: var(--t-meta);
		font-variant-numeric: tabular-nums;
		color: var(--ink-muted);
	}
</style>
