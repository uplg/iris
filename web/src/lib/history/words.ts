// What a watch is, said in words the same way on History and Admin: how far someone got,
// what exactly they watched (days and lengths: `@iris/api/format`).

import { clock, episodeCode, fileName, percent } from '@iris/api/format';

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
