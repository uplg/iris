// What a watch, a session or a date is, said in words the same way on History, Account and
// Admin: a day as people say it, how far someone got, how long they have been watching.

import { clock, duration, episodeCode, percent } from '@iris/api/format';

const DAY = 86_400_000;
const midnight = (ms: number) => new Date(ms).setHours(0, 0, 0, 0);
const time = (d: Date) => d.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });

/** A moment as people say it, past or future: « today at 21:04 », « yesterday at 09:12 »,
 * « tomorrow at 08:00 », « on Monday », « on 2 Oct », « on 2 Oct 2025 ». */
export function onDay(iso: string | number, now = Date.now()): string {
	const d = new Date(iso);
	const days = Math.round((midnight(d.getTime()) - midnight(now)) / DAY);
	if (days === 0) return `today at ${time(d)}`;
	if (days === -1) return `yesterday at ${time(d)}`;
	if (days === 1) return `tomorrow at ${time(d)}`;
	if (Math.abs(days) < 7) return `on ${d.toLocaleDateString('en-GB', { weekday: 'long' })}`;
	const sameYear = d.getFullYear() === new Date(now).getFullYear();
	return `on ${d.toLocaleDateString('en-GB', { day: 'numeric', month: 'short', ...(sameYear ? {} : { year: 'numeric' }) })}`;
}

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

/** How long since `iso`: « for 12 min », « for 1 h 5 min », « for under a minute ». */
export function since(iso: string, now = Date.now()): string {
	const secs = Math.max(0, (now - new Date(iso).getTime()) / 1000);
	return secs < 60 ? 'for under a minute' : `for ${duration(secs)}`;
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

/** A file's own name, without its folders. */
export function fileName(path: string | null | undefined): string | null {
	return path ? (path.split('/').pop() ?? path) : null;
}

/** « 1 title », « 3 titles ». */
export function count(n: number, one: string, many = `${one}s`): string {
	return `${n} ${n === 1 ? one : many}`;
}
