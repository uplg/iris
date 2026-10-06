<script lang="ts">
	// The titles on disk as a poster grid: found by name, filtered by type and state, sorted.
	// Each card says its state in words. A ghost (every release reclaimed, but you watched it)
	// stays greyed and labelled, and you can hide it from your own library.
	import { createQuery } from '@tanstack/svelte-query';
	import FindField from '#lib/components/FindField.svelte';
	import { me, tmdbImage, type CollectionListItem } from '@iris/api/client';
	import { plural } from '@iris/api/format';
	import { loadable } from '#lib/query.ts';
	import { stored, text } from '#lib/stored.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import { refocus } from '#lib/focus.ts';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import Loaded from '#lib/components/Loaded.svelte';
	import PosterCard from '#lib/components/PosterCard.svelte';
	import Select from '#lib/components/Select.svelte';
	import PillChoice from '#lib/components/PillChoice.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import {
		LIBRARY_SORT_KEY,
		SORTS,
		activityByCollection,
		filterTitles,
		titleMeta,
		titleStatus,
		type ShowFilter,
		type Sort,
		type TypeFilter
	} from './model.ts';
	import { collectionsOf, read, refreshLibrary, torrentsOf } from '#lib/queries.ts';
	import { resumeOf } from '#lib/watched.ts';

	const id = $props.id();
	const collections = createQuery(() => read.collections());
	const torrents = createQuery(() => read.torrents());
	const value = loadable(collections);
	const g = new Gesture();

	const keptSort = stored<Sort>(LIBRARY_SORT_KEY, 'recent', text(SORTS.map((s) => s.value)));
	let query = $state('');
	let type = $state<TypeFilter>('all');
	let show = $state<ShowFilter>('all');
	let sort = $state<Sort>(keptSort.get());

	const all = $derived(collectionsOf(collections.data));
	const activity = $derived(activityByCollection(torrentsOf(torrents.data)));
	const downloading = $derived(all.filter((c) => activity.get(c.id)?.fetching.length).length);
	const ghosts = $derived(all.filter((c) => c.ghost).length);

	const TYPES: readonly { value: TypeFilter; label: string }[] = [
		{ value: 'all', label: 'All' },
		{ value: 'movie', label: 'Movies' },
		{ value: 'series', label: 'Series' },
		{ value: 'anime', label: 'Anime' }
	];
	// only the states some title is in: a choice that would show nothing is not offered
	const shows = $derived(
		[
			{ value: 'all' as const, label: 'Everything' },
			downloading > 0 && { value: 'downloading' as const, label: `Downloading (${downloading})` },
			ghosts > 0 && { value: 'gone' as const, label: `No longer on disk (${ghosts})` }
		].filter((o): o is { value: ShowFilter; label: string } => !!o)
	);
	// a state no title is in any more (the downloads finished) falls back to everything
	const showing = $derived(shows.some((o) => o.value === show) ? show : 'all');
	const shown = $derived(filterTitles(all, { query, type, show: showing, sort }, activity));
	const filtered = $derived(query.trim() !== '' || type !== 'all' || showing !== 'all');

	function setSort(next: Sort) {
		sort = next;
		keptSort.set(next === 'recent' ? undefined : next);
	}

	function clear() {
		query = '';
		type = 'all';
		show = 'all';
		void refocus(`#${id}-search`);
	}

	function hide(c: CollectionListItem) {
		return g.run(
			() => me.dismissGone({ collection_id: c.id }),
			async () => {
				await refreshLibrary();
				ui.toast(`${c.display_title} is hidden from your library. Watching it again brings it back.`);
				await refocus(`#${id}-heading`, 'main h1');
			},
			c.id
		);
	}
</script>

<section class="titles" aria-labelledby="{id}-heading">
	<h2 id="{id}-heading" class="sr-only">Titles</h2>
	<Loaded
		{value}
		empty={all.length === 0}
		emptyText="Nothing in the library yet"
		emptyHint="Search for a title to add the first one."
		skeletons={3}
	>
		<div class="filters">
			<FindField id="{id}-search" label="Find a title" bind:value={query} />
			<PillChoice legend="Type" options={TYPES} value={type} onchange={(v) => (type = v)} />
			{#if shows.length > 1}
				<PillChoice legend="Show" options={shows} value={showing} onchange={(v) => (show = v)} />
			{/if}
			<div class="sort">
				<Select label="Sort" value={sort} options={SORTS} onchange={setSort} />
			</div>
		</div>

		<div class="count">
			<p role="status" class="hint">
				{shown.length === all.length ? plural(all.length, 'title') : `Showing ${shown.length} of ${all.length} titles`}
			</p>
			{#if filtered}<button class="link-btn" onclick={clear}>Clear filters</button>{/if}
		</div>

		{#if shown.length === 0}
			<div class="empty">
				<p>No title matches these filters.</p>
				<button class="btn" onclick={clear}>Clear filters</button>
			</div>
		{:else}
			<ul class="poster-grid">
				{#each shown as c (c.id)}
					<PosterCard
						href="/collection/{c.id}"
						title={c.display_title}
						art={tmdbImage(c.poster_path, 'w342')}
						meta={titleMeta(c)}
						status={titleStatus(c, activity.get(c.id))}
						progress={resumeOf(c.watch)?.share ?? undefined}
						actions={c.ghost ? ghostActions : undefined}
					/>
					{#snippet ghostActions()}
						<button
							class="icon-btn hide ghost-card"
							aria-label="Hide {c.display_title} from my library"
							title="Hide from my library (your history is kept)"
							{...pending(g.is(c.id))}
							onclick={() => hide(c)}
						>
							<Icon name="x" busy={g.is(c.id)} />
						</button>
					{/snippet}
				{/each}
			</ul>
		{/if}
	</Loaded>
</section>

<style>
	.titles {
		display: grid;
		gap: var(--s-4);
	}
	.filters {
		display: flex;
		flex-wrap: wrap;
		align-items: flex-end;
		gap: var(--s-4);
		margin-bottom: var(--s-4);
	}
	.sort {
		flex: 0 1 14rem;
		min-width: min(12rem, 100%);
	}
	.count {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--s-1) var(--s-3);
		margin-bottom: var(--s-4);
	}
	.count .link-btn {
		min-height: var(--control-h);
	}
	/* a long library: cards out of view are neither laid out nor painted (no timer, no
	   windowing script), their place kept at a card's height. That containment clips paint to
	   the card's box, so the card holds the art's outline inside its padding (the margin gives
	   the room back to the grid) */
	.poster-grid > :global(li) {
		content-visibility: auto;
		contain-intrinsic-size: auto 22rem;
		padding: var(--s-2);
		margin: calc(var(--s-2) * -1);
	}
	/* a ghost: greyed, its state said in words under it */
	.poster-grid > :global(li:has(.ghost-card) .art) {
		opacity: var(--disabled-opacity);
		filter: grayscale(1);
	}
	.hide {
		flex: none;
		width: var(--control-h);
		height: var(--control-h);
	}
</style>
