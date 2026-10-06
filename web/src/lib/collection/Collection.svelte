<script lang="ts">
	// A title of the library, a series or a movie: its head (artwork, facts, what to do), what is
	// on disk and the languages the next episodes start with, then its episodes (or its files)
	// and what used to be on disk. A movie with one copy goes straight to the player: there is
	// nothing to choose here. Read again every few seconds while something downloads.
	import { createQuery } from '@tanstack/svelte-query';
	import { goto } from '$app/navigation';
	import { library, tmdbImage, type CollectionDetail, type TorrentView } from '@iris/api/client';
	import { loadable, queryClient } from '#lib/query.ts';
	import { pageTitle } from '#lib/title.ts';
	import Loaded from '#lib/components/Loaded.svelte';
	import Poster from '#lib/components/Poster.svelte';
	import { FAST, KEYS, moving, read } from '#lib/queries.ts';
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
			queryKey: KEYS.collection(id),
			queryFn: () => library.collection(id),
			refetchInterval: (query: { state: { data?: CollectionDetail } }) => (query.state.data?.torrents.some(moving) ? FAST : false)
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
	let noBackdrop = $state(false);
	const backdrop = $derived(tmdbImage(c?.backdrop_path, 'w1280'));
</script>

<svelte:head><title>{pageTitle(c?.display_title ?? 'Library')}</title></svelte:head>

{#if !c}<h1 class="sr-only" tabindex="-1">Library title</h1>{/if}
<Loaded value={loadable(q)} missing="This title is no longer in the library.">
	{#if c}
		<nav class="crumbs" aria-label="Breadcrumb">
			<ol>
				<li><a href="/library">Library</a></li>
				<li aria-current="page">{c.display_title}</li>
			</ol>
		</nav>
		{#if leaving}
			<h1 class="sr-only" tabindex="-1">{c.display_title}</h1>
			<p class="hint">Opening the player…</p>
		{:else}
			<div class="stage">
				{#if backdrop && !noBackdrop}
					<img
						class="backdrop"
						src={backdrop}
						alt=""
						width="1280"
						height="720"
						fetchpriority="high"
						decoding="async"
						onerror={() => (noBackdrop = true)}
					/>
				{/if}
				<div class="top">
					<div class="poster"><Poster src={tmdbImage(c.poster_path, 'w342')} title={c.display_title} eager /></div>
					<Hero collection={c} meta={info.data} resume={resumeOf(c, watching.data)} {rows} />
					<aside class="side" aria-label="Releases and languages">
						<OnDisk collection={c} />
						{#if series}<Languages collectionId={c.id} title={c.display_title} {known} />{/if}
					</aside>
				</div>
			</div>
			<div class="below">
				{#if hasEpisodes}
					<Episodes collection={c} {rows} {torrents} />
					<!-- a pack the parser never split (episode 0) plays from its files -->
					{#if c.episodes.some((e) => e.episode === 0)}<RawFiles collection={c} />{/if}
				{:else if !(c.kind === 'movie' && c.torrents.length > 1)}
					<RawFiles collection={c} />
				{/if}
				{#if goneReleases.length}<GoneReleases collectionId={c.id} releases={goneReleases} />{/if}
			</div>
		{/if}
	{/if}
</Loaded>

<style>
	.crumbs ol {
		display: flex;
		flex-wrap: wrap;
		gap: var(--s-2);
		list-style: none;
		margin: 0;
		padding: var(--s-4) 0 0;
		font: var(--t-secondary);
		color: var(--ink-muted);
	}
	.crumbs li + li::before {
		content: '/';
		margin-right: var(--s-2);
		color: var(--ink-muted);
	}
	.crumbs a {
		display: inline-flex;
		align-items: center;
		min-height: var(--control-h);
		margin-block: calc(-1 * var(--s-3));
	}
	.crumbs li {
		display: inline-flex;
		align-items: center;
		min-width: 0;
		overflow-wrap: anywhere;
	}
	.stage {
		position: relative;
		isolation: isolate;
		padding-block: var(--s-5);
		margin-bottom: var(--s-5);
	}
	/* the backdrop: a faint picture behind the head, fading into the page */
	.backdrop {
		position: absolute;
		inset: 0;
		z-index: -1;
		width: 100%;
		height: 100%;
		object-fit: cover;
		opacity: 0.18;
		mask-image: linear-gradient(to bottom, black, transparent);
	}
	.top {
		display: flex;
		flex-wrap: wrap;
		gap: var(--s-5) var(--s-6);
		align-items: flex-start;
	}
	.poster {
		flex: 0 0 auto;
		width: min(220px, 45vw);
	}
	.side {
		flex: 1 1 18rem;
		max-width: 24rem;
		display: grid;
		gap: var(--s-4);
		min-width: 0;
	}
	.below {
		display: grid;
		gap: var(--s-6);
		min-width: 0;
	}
</style>
