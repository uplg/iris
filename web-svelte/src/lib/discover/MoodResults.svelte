<script lang="ts">
	// One mood's titles for a kind: what the catalogue and TMDB offer that the trackers carry,
	// recent first, tuned to the account. Its name comes from the board (cached from the tiles,
	// read on a deep link).
	import { createQuery } from '@tanstack/svelte-query';
	import { me, type MediaKind } from '@iris/api/client';
	import Loaded from '#lib/components/Loaded.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import { loadable } from '#lib/query.ts';
	import CatalogCard from '#lib/home/CatalogCard.svelte';
	import { KEYS, plural } from '#lib/home/data.ts';

	let { mood, kind }: { mood: string; kind: MediaKind } = $props();
	const board = createQuery(() => ({ queryKey: KEYS.moodBoard(kind), queryFn: () => me.moodBoard(kind) }));
	const results = createQuery(() => ({ queryKey: [...KEYS.moodResults, mood, kind], queryFn: () => me.moodResults(mood, kind) }));
	const label = $derived(board.data?.moods.find((m) => m.id === mood)?.label ?? 'This mood');
	const items = $derived(results.data?.items ?? []);
	const id = $props.id();
</script>

<section class="results" aria-labelledby="{id}-title">
	<a class="link-btn back" href="/discover?kind={kind}"><Icon name="arrow-left" />All moods</a>
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
		<ul class="grid">
			{#each items as card (card.catalog_id)}<CatalogCard {card} />{/each}
		</ul>
	</Loaded>
</section>

<style>
	.results {
		display: grid;
		gap: var(--s-3);
	}
	.back {
		display: inline-flex;
		align-items: center;
		gap: var(--s-2);
		justify-self: start;
		min-height: var(--control-h);
	}
	.head {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		gap: var(--s-2) var(--s-3);
	}
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(min(8.5rem, 100%), 1fr));
		gap: var(--s-5) var(--s-4);
		list-style: none;
		margin: 0;
		padding: 0;
	}
</style>
