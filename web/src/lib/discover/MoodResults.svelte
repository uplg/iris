<script lang="ts">
	// One mood's titles for a kind: what the catalogue and TMDB offer that the trackers carry,
	// recent first, tuned to the account. Its name comes from the board (cached from the tiles,
	// read on a deep link).
	import { createQuery } from '@tanstack/svelte-query';
	import BackLink from '#lib/components/BackLink.svelte';
	import type { MediaKind } from '@iris/api/client';
	import Loaded from '#lib/components/Loaded.svelte';
	import { loadable } from '#lib/query.ts';
	import CatalogCard from '#lib/home/CatalogCard.svelte';
	import { read } from '#lib/queries.ts';
	import { plural } from '@iris/api/format';

	let { mood, kind }: { mood: string; kind: MediaKind } = $props();
	const board = createQuery(() => read.moodBoard(kind));
	const results = createQuery(() => read.moodResults(mood, kind));
	const label = $derived(board.data?.moods.find((m) => m.id === mood)?.label ?? 'This mood');
	const items = $derived(results.data?.items ?? []);
	const id = $props.id();
</script>

<section class="results" aria-labelledby="{id}-title">
	<BackLink href="/discover?kind={kind}" label="All moods" />
	<div class="head">
		<h2 id="{id}-title" class="group-title">{label}</h2>
		{#if results.data}<span class="hint"
				>{plural(items.length, kind === 'tv' ? 'series' : 'movie', kind === 'tv' ? 'series' : 'movies')}</span
			>{/if}
	</div>
	<Loaded
		value={loadable(results)}
		empty={items.length === 0}
		emptyText="Nothing to get for this mood right now."
		emptyHint="Try another mood, or the other kind."
	>
		<ul class="poster-grid">
			{#each items as card (card.catalog_id)}<CatalogCard {card} />{/each}
		</ul>
	</Loaded>
</section>

<style>
	.results {
		display: grid;
		gap: var(--s-3);
	}
	.head {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		gap: var(--s-2) var(--s-3);
	}
</style>
