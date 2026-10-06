<script lang="ts">
	// A shelf of a live list: the row itself once there is something in it; before, under the
	// same title, the first load, a failure said with a retry, or why it is empty and what to
	// do (never a row that silently disappears). `hideEmpty`: a row that has nothing to say
	// when empty (a suggestion shelf) is left out instead.
	import type { Snippet } from 'svelte';
	import type { Loadable } from '#lib/query.ts';
	import Loaded from '#lib/components/Loaded.svelte';
	import Shelf from '#lib/components/Shelf.svelte';
	import Stale from '#lib/components/Stale.svelte';

	interface Props {
		title: string;
		fact?: string;
		href?: string;
		value: Loadable;
		/** How many cards there are (0: the empty state). */
		count: number;
		emptyText?: string;
		/** The way out of the empty state (a link to search). */
		emptyHint?: Snippet;
		hideEmpty?: boolean;
		/** The cards, as `<li>`s. */
		children: Snippet;
	}
	let { title, fact, href, value, count, emptyText, emptyHint, hideEmpty = false, children }: Props = $props();
	const id = $props.id();
	const ready = $derived(!value.loading && !value.failed);
</script>

{#if ready && count > 0}
	<Shelf {title} {fact} {href}>{@render children()}</Shelf>
	<Stale {value} />
{:else if !(ready && hideEmpty)}
	<section class="row" aria-labelledby="{id}-title">
		<div class="head">
			<h2 id="{id}-title">{title}</h2>
			{#if href}<a class="link-btn" {href}>See all</a>{/if}
		</div>
		{#if ready}
			<div class="empty">
				{#if emptyText}<p>{emptyText}</p>{/if}
				{#if emptyHint}<p class="hint">{@render emptyHint()}</p>{/if}
			</div>
		{:else}
			<Loaded {value}>{@render nothing()}</Loaded>
		{/if}
	</section>
{/if}

{#snippet nothing()}{/snippet}

<style>
	.row {
		display: grid;
		gap: var(--s-3);
	}
	.head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--s-3);
		min-height: var(--control-h);
	}
	h2 {
		font: var(--t-group);
		margin: 0;
	}
</style>
