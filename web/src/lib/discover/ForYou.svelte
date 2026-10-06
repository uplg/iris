<script lang="ts">
	// Every suggestion shelf (the home shows a few): what is trending, checked against the
	// trackers. Then where the trends come from, as TMDB's terms ask.
	import { createQuery } from '@tanstack/svelte-query';
	import Loaded from '#lib/components/Loaded.svelte';
	import Shelf from '#lib/components/Shelf.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import { loadable } from '#lib/query.ts';
	import CatalogCard from '#lib/home/CatalogCard.svelte';
	import { read } from '#lib/queries.ts';

	const page = createQuery(() => read.forYouPage());
	const shelves = $derived((page.data?.shelves ?? []).filter((s) => s.items.length > 0));
</script>

<Loaded
	value={loadable(page)}
	empty={shelves.length === 0}
	emptyText="Nothing suggested yet."
	emptyHint="Discovery follows what is trending and checks it against your trackers every few hours. Check back soon."
>
	{#each shelves as shelf (shelf.key)}
		<Shelf title={shelf.title}>
			{#each shelf.items as card (card.catalog_id)}<CatalogCard {card} />{/each}
		</Shelf>
	{/each}
</Loaded>

<p class="hint credit">
	Trends from {@render out('https://www.themoviedb.org', 'TMDB')} and {@render out('https://simkl.com', 'SIMKL')}, matched against your
	trackers. This product uses the TMDB API but is not endorsed or certified by TMDB.
</p>

{#snippet out(href: string, name: string)}
	<a {href} target="_blank" rel="noreferrer">{name}<Icon name="external-link" size={14} label="(opens in a new tab)" /></a>
{/snippet}

<style>
	.credit a {
		display: inline-flex;
		align-items: center;
		gap: var(--s-1);
	}
</style>
