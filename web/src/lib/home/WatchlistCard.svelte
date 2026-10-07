<script lang="ts">
	// A followed series: its poster (« 3 new » on its corner when episodes came out since the
	// last visit), and a line while one downloads. Leaving the watchlist goes through the server; the follow
	// comes back on its own with the next episode got or played, which the toast says.
	import { me, tmdbImage, type WatchlistItem } from '@iris/api/client';
	import { percent } from '@iris/api/format';
	import PosterCard from '#lib/components/PosterCard.svelte';
	import { Gesture } from '#lib/gesture.svelte.ts';
	import { queryClient } from '#lib/query.ts';
	import { ui } from '#lib/ui.svelte.ts';
	import { refocus, sectionHeading } from '#lib/focus.ts';
	import CardMenu from './CardMenu.svelte';
	import { KEYS } from '#lib/queries.ts';

	let { item, downloading }: { item: WatchlistItem; downloading?: number } = $props();
	const g = new Gesture();
	let trigger = $state<HTMLElement | null>(null);

	// a line only while something downloads; new episodes sit on the poster's corner, and
	// nothing new says nothing (a row of « No new episodes » was noise)
	const status = $derived(downloading !== undefined ? { tone: 'busy' as const, text: `Downloading · ${percent(downloading)}` } : undefined);
	const badge = $derived(item.new_count > 0 ? `${item.new_count} new` : undefined);

	function leave() {
		const heading = sectionHeading(trigger);
		return g.run(
			() => me.removeFromWatchlist(item.normalized_name),
			async () => {
				await Promise.all([
					queryClient.invalidateQueries({ queryKey: KEYS.watchlist }),
					queryClient.invalidateQueries({ queryKey: KEYS.summary })
				]);
				ui.toast(`${item.name} left your watchlist. It comes back when you get or play one of its episodes.`);
				await refocus(heading, 'main h1');
			}
		);
	}
</script>

<PosterCard href="/collection/{item.id}" title={item.name} art={tmdbImage(item.poster_path, 'w342')} meta="Series" {status} {badge}>
	{#snippet actions()}
		<CardMenu label="More for {item.name}" items={[{ label: 'Remove from your watchlist', run: leave }]} busy={g.is()} bind:ref={trigger} />
	{/snippet}
</PosterCard>
