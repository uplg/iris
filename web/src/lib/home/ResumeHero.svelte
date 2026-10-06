<script lang="ts">
	// The first thing to resume: where it stopped, what is left, the one button that plays it
	// from there, the series' episodes (a film has none), starting over; and the languages the
	// play will use. A tile whose file is not on disk gets it first, then plays it while it
	// downloads.
	import { createQuery } from '@tanstack/svelte-query';
	import { tmdbImage, type ContinueWatchingItem } from '@iris/api/client';
	import { clock, duration, kindLabel, percent, prettySceneName, timeLeft } from '@iris/api/format';
	import Icon from '#lib/components/Icon.svelte';
	import Progress from '#lib/components/Progress.svelte';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import Hero from './Hero.svelte';
	import { languagesLine, secondsLeft, watched } from './data.ts';
	import { read } from '#lib/queries.ts';
	import { tmdbMeta } from '#lib/tmdb.svelte.ts';
	import { getAndPlay, nextName, startOver, tileKey } from './continue.ts';
	import { watchHref } from '#lib/paths.ts';

	let { item }: { item: ContinueWatchingItem } = $props();
	const md = tmdbMeta(() => ({ id: item.tmdb_id, kind: item.kind, trusted: item.tmdb_verified }));
	const prefs = createQuery(() => read.playbackPrefs(item.collection_id ?? null));
	const g = new Gesture();

	const title = $derived(md.data?.title ?? prettySceneName(item.torrent_name));
	const left = $derived(secondsLeft(item));
	const share = $derived(watched(item));
	// a resume, not a fresh start: there is a position worth keeping
	const resuming = $derived(!item.grabbable && !item.next_up && item.position_seconds >= 5);
	const meta = $derived(
		item.kind === 'tv' && item.season !== null && item.season !== undefined
			? [
					`Season ${item.season}`,
					item.episode !== null && item.episode !== undefined ? `Episode ${item.episode}` : null,
					item.duration_seconds ? duration(item.duration_seconds) : null
				]
			: [md.data?.year ? String(md.data.year) : null, kindLabel(item.kind), item.duration_seconds ? duration(item.duration_seconds) : null]
	);
	const getting = $derived(g.is(`get:${tileKey(item)}`));
	const languages = $derived(languagesLine(prefs.data, item.kind));
</script>

<Hero
	eyebrow={item.next_up || item.grabbable ? 'Up next' : 'Continue where you left off'}
	{title}
	{meta}
	overview={md.data?.overview}
	art={tmdbImage(md.data?.backdrop_path, 'w1280')}
>
	{#snippet progress()}
		{#if resuming && share !== null && left !== null}
			<div class="left">
				<Progress label={timeLeft(left)} value={share * 100} max={100} valueText="{percent(share * 100)} watched" />
			</div>
		{/if}
	{/snippet}
	{#snippet actions()}
		{#if item.grabbable}
			<button class="btn primary" {...pending(getting)} onclick={() => getAndPlay(g, item)}>
				<Icon name="play" busy={getting} />{getting ? `Getting ${nextName(item)}…` : `Play ${nextName(item)}`}
			</button>
		{:else if resuming}
			<a class="btn primary" href={watchHref(item.infohash, item.file_idx)}><Icon name="play" />Resume at {clock(item.position_seconds)}</a>
		{:else}
			<a class="btn primary" href={watchHref(item.infohash, item.file_idx)}
				><Icon name="play" />{item.kind === 'tv' ? `Play ${nextName(item)}` : 'Play'}</a
			>
		{/if}
		{#if item.collection_id && item.kind === 'tv'}
			<a class="btn" href="/collection/{item.collection_id}"><Icon name="list" />All episodes</a>
		{/if}
		{#if resuming}
			<button class="btn ghost" {...pending(g.is(`over:${tileKey(item)}`))} onclick={() => startOver(g, item)}>
				<Icon name="rotate-ccw" busy={g.is(`over:${tileKey(item)}`)} />Start over
			</button>
		{/if}
	{/snippet}
	{#snippet footer()}
		{#if languages}<p class="hint"><Icon name="languages" size={16} />{languages}</p>{/if}
	{/snippet}
</Hero>

<style>
	.left {
		max-width: 24rem;
	}
	.hint {
		display: flex;
		align-items: center;
		gap: var(--s-2);
	}
</style>
