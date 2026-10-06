<script lang="ts">
	// Every release on the server, by what it is doing: downloading, needing a hand (stalled, an
	// error, paused by its tracker's policy), seeding. Found by name, hash or who added it.
	import type { CollectionListItem, ContinueWatchingItem, TorrentView } from '@iris/api/client';
	import FindField from '#lib/components/FindField.svelte';
	import { plural, speed } from '@iris/api/format';
	import { loadable } from '#lib/query.ts';
	import { refocus } from '#lib/focus.ts';
	import Loaded from '#lib/components/Loaded.svelte';
	import Group from '#lib/components/Group.svelte';
	import { createQuery } from '@tanstack/svelte-query';
	import { collectionsOf, read, torrentsOf } from '#lib/queries.ts';
	import { GROUPS, groupOf, releaseTitle, type Group as GroupId } from './model.ts';
	import TorrentRow from './TorrentRow.svelte';

	const id = $props.id();
	const torrents = createQuery(() => read.torrents());
	const collections = createQuery(() => read.collections());
	const watching = createQuery(() => read.continueWatching());
	const value = loadable(torrents);

	let query = $state('');

	const all = $derived(torrentsOf(torrents.data));
	const byCollection = $derived(new Map<string, CollectionListItem>(collectionsOf(collections.data).map((c) => [c.id, c])));
	const titleOf = (t: TorrentView) => releaseTitle(t, t.collection_id ? byCollection.get(t.collection_id) : undefined);
	const shown = $derived.by(() => {
		const q = query.trim().toLowerCase();
		if (!q) return all;
		return all.filter((t) => [t.name ?? '', t.infohash, t.added_by_name, titleOf(t)].some((s) => s.toLowerCase().includes(q)));
	});
	const grouped = $derived(
		GROUPS.map((g) => ({ ...g, items: shown.filter((t) => groupOf(t) === g.id) })).filter((g) => g.items.length > 0)
	);

	// per release, per file: the caller's watch state (one lookup per row)
	const watched = $derived.by(() => {
		const map = new Map<string, Map<number, ContinueWatchingItem>>();
		for (const w of watching.data ?? []) {
			if (!w.infohash) continue;
			const files = map.get(w.infohash) ?? new Map<number, ContinueWatchingItem>();
			files.set(w.file_idx, w);
			map.set(w.infohash, files);
		}
		return map;
	});

	const down = $derived(all.reduce((s, t) => s + t.download_speed_bps, 0));
	const up = $derived(all.reduce((s, t) => s + t.upload_speed_bps, 0));

	function fact(g: GroupId, items: TorrentView[]): string {
		const n = plural(items.length, 'release');
		if (g === 'downloading') return `${n} · ${speed(items.reduce((s, t) => s + t.download_speed_bps, 0))} down`;
		if (g === 'seeding') return `${n} · ${speed(items.reduce((s, t) => s + t.upload_speed_bps, 0))} up`;
		return n;
	}

	/** A row deleted: the focus goes to its group's title, else the next group's, else the page's. */
	const afterDelete = (group: GroupId) => refocus(`#${id}-${group}`, ...GROUPS.map((g) => `#${id}-${g.id}`), `#${id}-heading`, 'main h1');
</script>

<section class="downloads" aria-labelledby="{id}-heading">
	<h2 id="{id}-heading" class="sr-only" tabindex="-1">Downloads and seeding</h2>
	<Loaded
		{value}
		empty={all.length === 0}
		emptyText="Nothing is downloading or seeding"
		emptyHint="Releases appear here once a title is added."
		skeletons={3}
	>
		<!-- what was sent in all and the ratio are in the Seeding tile above -->
		<p class="totals hint">{`Right now ${speed(down)} down · ${speed(up)} up · ${all.length} active`}</p>

		<div class="filter">
			<FindField id="{id}-filter" label="Find a release" bind:value={query} hint="By title, release name, hash or who added it" />
			<p role="status" class="hint count">
				{shown.length === all.length ? plural(all.length, 'release') : `Showing ${shown.length} of ${all.length} releases`}
			</p>
		</div>

		{#if grouped.length === 0}
			<div class="empty">
				<p>No release matches “{query.trim()}”.</p>
				<button class="btn" onclick={() => ((query = ''), void refocus(`#${id}-filter`))}>Clear the search</button>
			</div>
		{/if}
		{#each grouped as g (g.id)}
			<Group id="{id}-{g.id}" title={g.title} fact={fact(g.id, g.items)}>
				<ul class="plain-list rows">
					{#each g.items as t (t.infohash)}
						<TorrentRow
							{t}
							title={titleOf(t)}
							collection={t.collection_id ? byCollection.get(t.collection_id) : undefined}
							watched={watched.get(t.infohash)}
							ondeleted={() => afterDelete(g.id)}
						/>
					{/each}
				</ul>
			</Group>
		{/each}
	</Loaded>
</section>

<style>
	.downloads {
		display: grid;
		gap: var(--s-4);
	}
	.totals {
		font-variant-numeric: tabular-nums;
		margin-bottom: var(--s-3);
	}
	.filter {
		display: flex;
		flex-wrap: wrap;
		align-items: flex-end;
		gap: var(--s-2) var(--s-4);
		margin-bottom: var(--s-4);
	}
	.count {
		font-variant-numeric: tabular-nums;
	}
	.rows {
		display: grid;
	}
	/* a long list: rows out of view are neither laid out nor painted, their place kept. That
	   clips paint to the row, so its side padding holds the art's outline */
	.rows > :global(li) {
		content-visibility: auto;
		contain-intrinsic-size: auto 10rem;
		padding-inline: var(--s-2);
		margin-inline: calc(var(--s-2) * -1);
	}
</style>
