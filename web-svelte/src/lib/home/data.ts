// What the home and discover screens read, and the words they say about it: the query keys
// (shared with every view that invalidates them), the "Right now" facts, the downloads per
// collection, a title's kind and the playback languages, said once.

import type { ContinueWatchingItem, HomeSummary, MediaKind, PlaybackPrefs, TorrentView } from '@iris/api/client';
import { formatSize, percent, timeLeft } from '@iris/api/format';

export const KEYS = {
	continueWatching: ['continue-watching'],
	watchlist: ['watchlist'],
	summary: ['me', 'summary'],
	torrents: ['torrents'],
	collections: ['library', 'collections'],
	featured: ['discover-featured'],
	forYou: ['for-you'],
	forYouPage: ['for-you-page'],
	moodBoard: (kind: MediaKind) => ['mood-board', kind],
	moodResults: ['mood-results'],
	preferences: ['preferences'],
	genres: ['genres'],
	languages: ['languages'],
	playbackPrefs: (collectionId: string | null) => ['playback-prefs', collectionId]
} as const;

/** `1 download`, `3 downloads`. */
export function plural(n: number, one: string, many = `${one}s`): string {
	return `${n} ${n === 1 ? one : many}`;
}

const known = (n: number | null | undefined): n is number => typeof n === 'number' && Number.isFinite(n);

/** The home's "Right now" line, in words: only what is happening. */
export function rightNow(s: HomeSummary): string[] {
	const facts: string[] = [];
	if (s.downloading > 0) {
		const eta = known(s.downloading_eta_seconds) && s.downloading_eta_seconds > 0 ? ` · ${timeLeft(s.downloading_eta_seconds)}` : '';
		facts.push(`${plural(s.downloading, 'download')} · ${percent(s.downloading_pct)}${eta}`);
	}
	if (s.new_episodes > 0) facts.push(`${plural(s.new_episodes, 'new episode')} on your watchlist`);
	if (s.disk) facts.push(`${formatSize(s.disk.free_bytes)} free on disk`);
	if (s.seeding > 0) facts.push(`Seeding ${plural(s.seeding, 'release')}`);
	return facts;
}

/** What is still downloading in each collection, as a share of its bytes (0 to 100). */
export function downloadsByCollection(list: readonly TorrentView[]): Map<string, number> {
	const sums = new Map<string, { done: number; total: number }>();
	for (const t of list) {
		if (t.finished || !t.collection_id) continue;
		const s = sums.get(t.collection_id) ?? { done: 0, total: 0 };
		s.done += t.progress_bytes;
		s.total += t.total_size_bytes;
		sums.set(t.collection_id, s);
	}
	return new Map([...sums].map(([id, s]) => [id, s.total > 0 ? (s.done / s.total) * 100 : 0]));
}

/** `Movie`, `Series`, `Anime · Series`. */
export function kindLabel(kind: MediaKind | null | undefined, anime = false): string {
	const k = kind === 'tv' ? 'Series' : 'Movie';
	return anime ? `Anime · ${k}` : k;
}

/** Seconds left to watch, when the length is known. */
export function secondsLeft(it: Pick<ContinueWatchingItem, 'duration_seconds' | 'position_seconds'>): number | null {
	return known(it.duration_seconds) && it.duration_seconds > 0 ? Math.max(0, it.duration_seconds - it.position_seconds) : null;
}

/** Watched so far, 0 to 1, when the length is known. */
export function watched(it: Pick<ContinueWatchingItem, 'duration_seconds' | 'position_seconds'>): number | null {
	return known(it.duration_seconds) && it.duration_seconds > 0 ? Math.min(1, it.position_seconds / it.duration_seconds) : null;
}

function languageName(code: string): string {
	try {
		return new Intl.DisplayNames(['en'], { type: 'language' }).of(code) ?? code;
	} catch {
		return code;
	}
}

/** The languages a play will use, when the account (or the series) chose them. */
export function languagesLine(p: PlaybackPrefs | undefined): string | null {
	if (!p) return null;
	const parts: string[] = [];
	if (p.audio_language) parts.push(`audio in ${languageName(p.audio_language)}`);
	if (p.subtitle_language === 'off') parts.push('subtitles off');
	else if (p.subtitle_language) parts.push(`subtitles in ${languageName(p.subtitle_language)}`);
	if (parts.length === 0) return null;
	return `Plays with ${parts.join(', ')}${p.for_collection ? ', as chosen for this series' : ''}.`;
}

/** A TMDB still wide enough for a 16:9 frame (`tmdbImage` stops at w500). */
export function stillUrl(path: string | null | undefined, size: 'w780' | 'w1280' = 'w780'): string | null {
	return path ? `https://image.tmdb.org/t/p/${size}${path}` : null;
}
