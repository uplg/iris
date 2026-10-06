// What one can do with a Continue Watching tile, one path for the hero and every card: play,
// getting the file first when it is not on disk (it plays while it downloads); take the tile
// off the row; mark it watched; start over. Each goes to the server, then the row is read
// again: nothing leaves the screen before the server said so.

import { goto } from '$app/navigation';
import { library, me, progress, type ContinueWatchingItem } from '@iris/api/client';
import { episodeCode } from '@iris/api/format';
import { queryClient } from '#lib/query.ts';
import { ui } from '#lib/ui.svelte.ts';
import { errorText } from '#lib/errors.ts';
import { FAILURE, haptic } from '#lib/haptics.ts';
import { refocus } from '#lib/focus.ts';
import type { Gesture } from '#lib/gesture.svelte.ts';
import { KEYS } from '#lib/queries.ts';
import { watchHref } from '#lib/paths.ts';

/** A tile's identity: tiles not on disk all carry an empty infohash, their series tells them apart. */
export const tileKey = (it: ContinueWatchingItem) => (it.collection_id ? `c:${it.collection_id}` : `${it.infohash}:${it.file_idx}`);

/** Where the tile's name leads: the player, or the series when there is nothing to play yet. */
export const tileHref = (it: ContinueWatchingItem) =>
	it.grabbable ? (it.collection_id ? `/collection/${it.collection_id}` : '/library') : watchHref(it.infohash, it.file_idx);

/** `S2:E5`, or "the next episode" when the server does not know which. */
export const nextName = (it: ContinueWatchingItem) => episodeCode(it.season, it.episode) ?? 'the next episode';

const reread = () => queryClient.invalidateQueries({ queryKey: KEYS.continueWatching });

/** Gets the tile's episode (the series' usual language) and plays it. Failing, the series page
 * lists every release: the toast leads there. */
export function getAndPlay(g: Gesture, it: ContinueWatchingItem) {
	const cid = it.collection_id;
	if (!cid || it.season === null || it.season === undefined || it.episode === null || it.episode === undefined) {
		return goto(tileHref(it));
	}
	const season = it.season;
	const episode = it.episode;
	return g.run(
		() => library.grabCollectionEpisode(cid, season, episode, 'auto'),
		async (got) => {
			void reread();
			await goto(watchHref(got.infohash, got.file_idx));
		},
		`get:${tileKey(it)}`,
		{
			refused: (e) => {
				haptic(FAILURE);
				ui.toast(`Could not get ${nextName(it)}. ${errorText(e)}`, {
					warn: true,
					action: { label: 'Open the series', run: () => goto(`/collection/${cid}`) }
				});
				return true;
			}
		}
	);
}

/** Takes the tile off the row: a series as a whole (until a newer episode plays), a movie by
 * its file. The focus goes to the row's title once it is gone. */
export function removeTile(g: Gesture, it: ContinueWatchingItem, title: string, heading: HTMLElement | null) {
	const body = it.collection_id ? { collection_id: it.collection_id } : { infohash: it.infohash, file_idx: it.file_idx };
	return g.run(
		() => me.dismissContinueWatching(body),
		async () => {
			await reread();
			ui.say(`${title} removed from Continue watching`);
			await refocus(heading, 'main h1');
		},
		`remove:${tileKey(it)}`
	);
}

export function markWatched(g: Gesture, it: ContinueWatchingItem, title: string, heading: HTMLElement | null) {
	return g.run(
		() => progress.markWatched(it.infohash, it.file_idx),
		async () => {
			await reread();
			ui.say(`${title} marked as watched`);
			await refocus(heading, 'main h1');
		},
		`watched:${tileKey(it)}`
	);
}

/** Saves the position back to the start (a deliberate seek, so the server keeps it), then plays. */
export function startOver(g: Gesture, it: ContinueWatchingItem) {
	return g.run(
		() => progress.put(it.infohash, it.file_idx, { position_seconds: 0, duration_seconds: it.duration_seconds ?? null, seek: true }),
		async () => {
			void reread();
			await goto(watchHref(it.infohash, it.file_idx));
		},
		`over:${tileKey(it)}`
	);
}
