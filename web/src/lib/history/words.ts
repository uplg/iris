// What a watch is, said in words the same way on History and Admin: how far someone got,
// what exactly they watched (days and lengths: `@iris/api/format`).

import { clock, duration, episodeCode, fileName, percent, prettySceneName } from '@iris/api/format';

/** How far a watch went: « Watched to the end », « 42% watched, stopped at 32:10 »,
 * « Not started ». */
export function progressWords(position: number, total: number | null | undefined, completed = false): string {
	if (completed) return 'Watched to the end';
	if (position < 1) return 'Not started';
	if (!total || total <= 0) return `Stopped at ${clock(position)}`;
	return `${percent(Math.min(100, (position / total) * 100))} watched, stopped at ${clock(position)}`;
}

/** The share watched, 0 to 1 (a bar beside the words). */
export function watchedShare(position: number, total: number | null | undefined, completed = false): number {
	if (completed) return 1;
	return total && total > 0 ? Math.min(1, Math.max(0, position / total)) : 0;
}

/** What exactly was watched: « Episode 1156 », « S2:E4 », « Season 2 », else the file's name. */
export function whatWatched(it: {
	absolute_episode?: number | null;
	season?: number | null;
	episode?: number | null;
	file_path?: string | null;
}): string | null {
	if (typeof it.absolute_episode === 'number') return `Episode ${it.absolute_episode}`;
	if (typeof it.season === 'number') return episodeCode(it.season, it.episode);
	return fileName(it.file_path);
}

/** How far a watch went, short: « Finished », « 42 min of 2 h 10 min », « Just started »,
 * « Stopped at 32:10 » (its length unknown). */
export function progressShort(position: number, total: number | null | undefined, completed = false): string {
	if (completed) return 'Finished';
	if (position < 60) return 'Just started';
	if (!total || total <= 0) return `Stopped at ${clock(position)}`;
	return `${duration(Math.min(position, total))} of ${duration(total)}`;
}

/** What a play is, as people name it: the title (the collection's, else the release's name
 * cleaned up), then what of it (« S2:E4 · Woe’s Hollow », « Episode 1156 », « Film · 2021 »). */
export function playName(it: {
	collection_title?: string | null;
	torrent_name?: string | null;
	file_path?: string | null;
	kind?: string | null;
	season?: number | null;
	episode?: number | null;
	absolute_episode?: number | null;
	episode_title?: string | null;
	year?: number | null;
}): { title: string; detail: string | null } {
	const raw = fileName(it.file_path) ?? it.torrent_name ?? '';
	const title = it.collection_title ?? (raw ? prettySceneName(it.torrent_name ?? raw) : 'Something unnamed');
	// a file of a pack the library could not place still names its episode
	const named = /\bS(\d{1,2})E(\d{1,3})\b/i.exec(raw);
	const code =
		typeof it.absolute_episode === 'number'
			? `Episode ${it.absolute_episode}`
			: (episodeCode(it.season, it.episode) ?? (named ? episodeCode(Number(named[1]), Number(named[2])) : null));
	if (code) return { title, detail: [code, it.episode_title].filter(Boolean).join(' · ') };
	if (it.kind === 'movie') return { title, detail: typeof it.year === 'number' ? `Film · ${it.year}` : 'Film' };
	return { title, detail: null };
}
