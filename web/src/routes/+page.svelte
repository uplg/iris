<script lang="ts">
	// Home: what to watch now (resume the last thing, else the library's freshest title, else
	// the tracker's featured release), what Iris is doing right now, then the rows: Continue
	// watching, the watchlist (fresh episodes first), the suggestions, the library. The search
	// lives on its own page; moods on Discover.
	import { createQuery } from '@tanstack/svelte-query';
	import { plural } from '@iris/api/format';
	import PageHead from '#lib/components/PageHead.svelte';
	import Shelf from '#lib/components/Shelf.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import { loadable } from '#lib/query.ts';
	import Row from '#lib/home/Row.svelte';
	import RightNow from '#lib/home/RightNow.svelte';
	import Onboarding from '#lib/home/Onboarding.svelte';
	import ResumeHero from '#lib/home/ResumeHero.svelte';
	import LibraryHero from '#lib/home/LibraryHero.svelte';
	import FeaturedHero from '#lib/home/FeaturedHero.svelte';
	import ContinueCard from '#lib/home/ContinueCard.svelte';
	import WatchlistCard from '#lib/home/WatchlistCard.svelte';
	import LibraryCard from '#lib/home/LibraryCard.svelte';
	import CatalogCard from '#lib/home/CatalogCard.svelte';
	import { tileKey } from '#lib/home/continue.ts';
	import { downloadsByCollection } from '#lib/home/data.ts';
	import { collectionsOf, read, torrentsOf } from '#lib/queries.ts';

	const LIBRARY_ROW = 12;

	const cw = createQuery(() => read.continueWatching());
	const watchlist = createQuery(() => ({
		...read.watchlist(),
		// fresh episodes first (a stable sort: the server's order otherwise)
		select: (items) => items.toSorted((a, b) => b.new_count - a.new_count)
	}));
	const forYou = createQuery(() => read.forYou());
	const collections = createQuery(() => ({
		...read.collections(),
		select: collectionsOf
	}));
	// what still downloads, per collection: read often only while something moves
	const transfers = createQuery(() => ({
		...read.torrents(),
		select: torrentsOf
	}));
	const downloads = $derived(downloadsByCollection(transfers.data ?? []));

	const resume = $derived(cw.data?.[0]);
	const libraryPick = $derived(collections.data?.find((c) => !c.ghost));
	// the tracker is asked only when there is nothing of one's own to show
	const featured = createQuery(() => ({
		...read.featured(),
		enabled: cw.isSuccess && collections.isSuccess && !resume && !libraryPick
	}));
	const featuredPick = $derived(featured.data?.movies[0] ?? featured.data?.series[0]);
	const libraryRow = $derived((collections.data ?? []).slice(0, LIBRARY_ROW));
</script>

<PageHead title="Iris" hidden />
<Onboarding />

{#if resume}
	{#key tileKey(resume)}<ResumeHero item={resume} />{/key}
{:else if libraryPick}
	{#key libraryPick.id}<LibraryHero item={libraryPick} />{/key}
{:else if featuredPick}
	<FeaturedHero result={featuredPick} />
{/if}

<div class="home">
	<RightNow />

	<Row
		title="Continue watching"
		value={loadable(cw)}
		count={cw.data?.length ?? 0}
		emptyText="Nothing to resume yet. What you start watching waits for you here."
	>
		{#each cw.data ?? [] as item (tileKey(item))}<ContinueCard {item} />{/each}
	</Row>

	<Row
		title="Your watchlist"
		fact={watchlist.data?.length ? plural(watchlist.data.length, 'series', 'series') : undefined}
		value={loadable(watchlist)}
		count={watchlist.data?.length ?? 0}
		emptyText="No series followed yet."
	>
		{#snippet emptyHint()}Find a series in <a href="/search">Search</a>: getting an episode follows it.{/snippet}
		{#each watchlist.data ?? [] as item (item.id)}<WatchlistCard {item} downloading={downloads.get(item.id)} />{/each}
	</Row>

	{#each forYou.data?.shelves ?? [] as shelf (shelf.key)}
		{#if shelf.items.length}
			<Shelf title={shelf.title} href="/discover">
				{#each shelf.items as card (card.catalog_id)}<CatalogCard {card} />{/each}
			</Shelf>
		{/if}
	{/each}

	<Row
		title="Your library"
		fact={collections.data?.length ? plural(collections.data.length, 'title') : undefined}
		href="/library"
		value={loadable(collections)}
		count={libraryRow.length}
		emptyText="Nothing in the library yet."
	>
		{#snippet emptyHint()}Start a <a href="/search">search</a> to add your first title.{/snippet}
		{#each libraryRow as item (item.id)}<LibraryCard {item} downloading={downloads.get(item.id)} />{/each}
	</Row>

	<p class="tonight">
		<Icon name="compass" />Not sure what to watch tonight? <a href="/discover">Pick a mood in Discover</a>
	</p>
</div>

<style>
	.home {
		display: grid;
		gap: var(--s-6);
		min-width: 0;
	}
	.tonight {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--s-2);
		margin: 0;
		color: var(--ink-muted);
	}
	.tonight a {
		display: inline-flex;
		align-items: center;
		min-height: var(--control-h);
	}
</style>
