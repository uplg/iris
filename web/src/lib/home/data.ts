// The words the home and discover screens say about what they read: the "Right now" facts,
// the downloads per collection, a title's kind and the playback languages, said once.

import type { ContinueWatchingItem, HomeSummary, MediaKind, PlaybackPrefs, TorrentView } from '@iris/api/client';
import { formatSize, percent, plural, thisTitle, timeLeft } from '@iris/api/format';
import { languageName } from '#lib/language.ts';
import { isFetching } from '#lib/torrent.ts';

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
		if (!isFetching(t) || !t.collection_id) continue;
		const s = sums.get(t.collection_id) ?? { done: 0, total: 0 };
		s.done += t.progress_bytes;
		s.total += t.total_size_bytes;
		sums.set(t.collection_id, s);
	}
	return new Map([...sums].map(([id, s]) => [id, s.total > 0 ? (s.done / s.total) * 100 : 0]));
}

/** Seconds left to watch, when the length is known. */
export function secondsLeft(it: Pick<ContinueWatchingItem, 'duration_seconds' | 'position_seconds'>): number | null {
	return known(it.duration_seconds) && it.duration_seconds > 0 ? Math.max(0, it.duration_seconds - it.position_seconds) : null;
}

/** The languages a play will use, when the account (or the title) chose them. */
export function languagesLine(p: PlaybackPrefs | undefined, kind: MediaKind | null | undefined): string | null {
	if (!p) return null;
	const parts: string[] = [];
	if (p.audio_language) parts.push(`audio in ${languageName(p.audio_language) ?? p.audio_language}`);
	if (p.subtitle_language === 'off') parts.push('subtitles off');
	else if (p.subtitle_language) parts.push(`subtitles in ${languageName(p.subtitle_language) ?? p.subtitle_language}`);
	if (parts.length === 0) return null;
	return `Plays with ${parts.join(', ')}${p.for_collection ? `, as chosen for ${thisTitle(kind)}` : ''}.`;
}
