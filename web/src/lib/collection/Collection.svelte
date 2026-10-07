<script lang="ts">
	// A title of the library, a series or a movie: its banner (artwork, facts, what to do), then
	// its episodes (or its files, a movie's copies) with, beside them on a wide screen, what is on
	// disk, the languages the next episodes start with and what used to be on disk. A movie with
	// one copy goes straight to the player: there is nothing to choose here. Read again every few
	// seconds while something downloads.
	import { createQuery } from '@tanstack/svelte-query';
	import { goto } from '$app/navigation';
	import type { CollectionDetail, TorrentView } from '@iris/api/client';
	import { loadable, queryClient } from '#lib/query.ts';
	import { pageTitle } from '#lib/title.ts';
	import Loaded from '#lib/components/Loaded.svelte';
	import BackLink from '#lib/components/BackLink.svelte';
	import { FAST, read } from '#lib/queries.ts';
	import { isMoving } from '#lib/torrent.ts';
	import { tmdbMeta } from '#lib/tmdb.svelte.ts';
	import Episodes from './Episodes.svelte';
	import GoneReleases from './GoneReleases.svelte';
	import Hero from './Hero.svelte';
	import Languages from './Languages.svelte';
	import OnDisk from './OnDisk.svelte';
	import RawFiles from './RawFiles.svelte';
	import { episodesOf, mainVideo, resumeOf } from './merge.ts';
	import { releaseCodes } from './lang.ts';
	import { watchHref } from '#lib/paths.ts';

	let { id }: { id: string } = $props();

	const q = createQuery(
		() => ({
			...read.collection(id),
			refetchInterval: (query: { state: { data?: CollectionDetail } }) => (query.state.data?.torrents.some(isMoving) ? FAST : false)
		}),
		() => queryClient
	);
	const c = $derived(q.data);

	const tmdb = $derived(c?.tmdb_id ?? null);
	const info = tmdbMeta(() => ({ id: tmdb, kind: c?.kind, trusted: true }));
	const watching = createQuery(read.continueWatching, () => queryClient);

	// only when the page opens: a copy deleted later down to one keeps the person here
	let decided = false;
	let leaving = $state(false);
	$effect(() => {
		if (!c || decided) return;
		decided = true;
		const only = c.kind === 'movie' && c.torrents.length === 1 ? c.torrents[0] : null;
		const f = only ? mainVideo(only) : undefined;
		if (only && f) {
			leaving = true;
			void goto(watchHref(only.infohash, f.index), { replace: true });
		}
	});

	const rows = $derived(c ? episodesOf(c) : []);
	const torrents = $derived(new Map<string, TorrentView>((c?.torrents ?? []).map((t) => [t.infohash, t])));
	const series = $derived(c?.kind === 'tv');
	// a ghost series (everything reclaimed) still shows its episodes, not the files
	const hasEpisodes = $derived(!!c && series && (c.episodes.length > 0 || !!c.available_episodes?.length || !!c.gone_episodes?.length));
	// a gone release whose episodes show in place needs no second row (a pack, episode 0, does)
	const goneReleases = $derived.by(() => {
		if (!c) return [];
		const inline = new Set((c.gone_episodes ?? []).filter((g) => g.episode > 0).map((g) => g.infohash));
		return (c.gone_releases ?? []).filter((r) => !inline.has(r.infohash));
	});
	const known = $derived(
		releaseCodes([...(c?.episodes ?? []).map((e) => e.language), ...(c?.available_episodes ?? []).map((e) => e.language)]).concat(
			info.data?.original_language ? [info.data.original_language] : []
		)
	);
	const movieCopies = $derived(c?.kind === 'movie' && c.torrents.length > 1);
</script>

<svelte:head><title>{pageTitle(c?.display_title ?? 'Library')}</title></svelte:head>

{#if !c}<h1 class="sr-only" tabindex="-1">Library title</h1>{/if}
<Loaded value={loadable(q)} missing="This title is no longer in the library.">
	{#if c}
		<BackLink href="/library" label="Library" />
		{#if leaving}
			<h1 class="sr-only" tabindex="-1">{c.display_title}</h1>
			<p class="hint">Opening the player…</p>
		{:else}
			<Hero collection={c} meta={info.data} resume={resumeOf(c, watching.data)} {rows} />
			<div class="body">
				<div class="main">
					{#if hasEpisodes}
						<Episodes collection={c} {rows} {torrents} />
						<!-- a pack the parser never split (episode 0) plays from its files -->
						{#if c.episodes.some((e) => e.episode === 0)}<RawFiles collection={c} />{/if}
					{:else if movieCopies}
						<OnDisk collection={c} watching={watching.data ?? []} />
					{:else}
						<RawFiles collection={c} />
					{/if}
				</div>
				{#if !movieCopies || goneReleases.length}
					<aside class="side" aria-label="Releases and languages">
						{#if !movieCopies}<OnDisk collection={c} watching={watching.data ?? []} />{/if}
						{#if series}<Languages collectionId={c.id} title={c.display_title} {known} />{/if}
						{#if goneReleases.length}<GoneReleases collectionId={c.id} title={c.display_title} releases={goneReleases} />{/if}
					</aside>
				{/if}
			</div>
		{/if}
	{/if}
</Loaded>

<style>
	.body {
		display: grid;
		grid-template-columns: minmax(0, 1fr);
		gap: var(--s-6);
		margin-top: var(--s-6);
	}
	.main,
	.side {
		display: grid;
		gap: var(--s-5);
		align-content: start;
		min-width: 0;
	}
	/* wide: the episodes keep the room, the rest a narrow column beside them */
	@media (min-width: 1200px) {
		.body:has(.side) {
			grid-template-columns: minmax(0, 1fr) minmax(20rem, 26rem);
		}
	}
</style>
