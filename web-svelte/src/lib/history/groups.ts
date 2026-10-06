// A watch history, grouped by what was watched: a series under its title with its episodes,
// a film on one line. A title whose every file was reclaimed from disk stays listed (a
// « ghost »): its title, its poster, how far one got, and the way back to it.

import type { HistoryItem, UserHistoryItem } from '@iris/api/client';

export type Item = HistoryItem | UserHistoryItem;

export interface Group {
	key: string;
	collectionId: string | null;
	title: string;
	/** The first verified TMDB poster among its rows. */
	posterPath: string | null;
	/** Every file of it is gone from disk. */
	ghost: boolean;
	/** One file with no episode to it (a film): drawn as one line. */
	solo: boolean;
	items: Item[];
}

export const itemKey = (it: Item) => `${it.infohash}:${it.file_idx}`;

/** The groups, in the order of each one's latest watch (the server sends newest first). */
export function groupHistory(items: readonly Item[]): Group[] {
	const groups: Group[] = [];
	const byKey = new Map<string, Group>();
	for (const it of items) {
		const key = it.collection_id ?? `solo:${it.infohash}`;
		let g = byKey.get(key);
		if (!g) {
			g = {
				key,
				collectionId: it.collection_id ?? null,
				title: it.collection_title ?? it.torrent_name,
				posterPath: null,
				ghost: true,
				solo: false,
				items: []
			};
			byKey.set(key, g);
			groups.push(g);
		}
		g.items.push(it);
		g.posterPath ??= it.poster_path ?? null;
		if (!it.deleted) g.ghost = false;
	}
	for (const g of groups) {
		const only = g.items[0];
		g.solo = g.items.length === 1 && (only.season ?? null) === null && (only.absolute_episode ?? null) === null;
	}
	return groups;
}

/** Gone from disk, and the release it came from is known: it can be downloaded again
 * (same release, same files: the saved position applies again). */
export const canRestore = (it: Item) => it.deleted && !!it.source_provider && !!it.source_external_id;
