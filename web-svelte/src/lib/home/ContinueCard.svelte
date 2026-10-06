<script lang="ts">
	// One Continue Watching tile: a 16:9 still, the title (its clean TMDB name once the server
	// trusts the match), the episode, what is left. A tile whose file is not on disk (the next
	// episode never downloaded, or reclaimed) leads to its series and offers to get it and play.
	import type { ContinueWatchingItem } from '@iris/api/client';
	import { episodeCode, prettySceneName, timeLeft } from '@iris/api/format';
	import PosterCard from '#lib/components/PosterCard.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import { Gesture, pending } from '#lib/gesture.svelte.ts';
	import { sectionHeading } from '#lib/focus.ts';
	import CardMenu, { type MenuItem } from './CardMenu.svelte';
	import { kindLabel, secondsLeft, stillUrl, watched } from './data.ts';
	import { tmdbMeta } from './tmdb.svelte.ts';
	import { getAndPlay, markWatched, nextName, removeTile, tileHref, tileKey } from './continue.ts';

	let { item }: { item: ContinueWatchingItem } = $props();
	const md = tmdbMeta(() => ({ id: item.tmdb_id, kind: item.kind, trusted: item.tmdb_verified }));
	const g = new Gesture();
	let trigger = $state<HTMLElement | null>(null);

	const title = $derived(md.data?.title ?? prettySceneName(item.torrent_name));
	const code = $derived(episodeCode(item.season, item.episode));
	const left = $derived(secondsLeft(item));
	const share = $derived(watched(item));
	const status = $derived.by(() => {
		if (item.grabbable) return { tone: 'available' as const, text: `Up next · ${nextName(item)} · Not downloaded` };
		if (item.next_up) return { tone: 'info' as const, text: 'Up next' };
		if (left !== null) return { tone: 'info' as const, text: timeLeft(left) };
		return undefined;
	});
	const items = $derived<MenuItem[]>([
		{ label: 'Remove from Continue watching', run: () => removeTile(g, item, title, sectionHeading(trigger)) },
		...(item.grabbable ? [] : [{ label: 'Mark as watched', run: () => markWatched(g, item, title, sectionHeading(trigger)) }])
	]);
	const getting = $derived(g.is(`get:${tileKey(item)}`));
</script>

<PosterCard
	href={tileHref(item)}
	{title}
	art={stillUrl(md.data?.backdrop_path)}
	shape="still"
	meta={code ?? kindLabel(item.kind)}
	{status}
	progress={!item.next_up && share !== null && share > 0 ? share : undefined}
>
	{#snippet actions()}
		{#if item.grabbable}
			<button
				class="icon-btn get"
				aria-label="Get {nextName(item)} of {title} and play"
				{...pending(getting)}
				onclick={() => getAndPlay(g, item)}
			>
				<Icon name="play" busy={getting} />
			</button>
		{/if}
		<CardMenu label="More for {title}" {items} busy={g.is() && !getting} bind:ref={trigger} />
	{/snippet}
</PosterCard>

<style>
	.get {
		flex: none;
		width: var(--control-h);
		height: var(--control-h);
		color: var(--accent);
	}
</style>
