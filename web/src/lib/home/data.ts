// The words the home and discover screens say about what they read: the "Right now" facts,
// the downloads per collection, a title's kind, said once.

import type { ContinueWatchingItem, FriendEvent, HomeSummary, ReleaseNote, TorrentView } from '@iris/api/client';
import { episodeCode, percent, plural, timeLeft } from '@iris/api/format';
import { isFetching } from '#lib/torrent.ts';

const known = (n: number | null | undefined): n is number => typeof n === 'number' && Number.isFinite(n);

/** The home's "Right now" line, in words: only what is happening. */
export function rightNow(s: HomeSummary): string[] {
	const facts: string[] = [];
	if (s.downloading > 0) {
		const eta = known(s.downloading_eta_seconds) && s.downloading_eta_seconds > 0 ? ` · ${timeLeft(s.downloading_eta_seconds)}` : '';
		facts.push(`${plural(s.downloading, 'download')} · ${percent(s.downloading_pct)}${eta}`);
	}
	return facts;
}

const DOES: Record<FriendEvent['action'], string> = { watching: 'is watching', watched: 'watched', added: 'added' };

/** What a friend does, in a sentence: « Muriel is watching Severance · S2:E4 ». */
export function friendLine(e: FriendEvent): string {
	const code = episodeCode(e.season, e.episode);
	return `${e.friend_name} ${DOES[e.action]} ${e.title}${code ? ` · ${code}` : ''}`;
}

/** The update sentence still to say to a browser that last saw `seen`: none on a first visit,
 * nor once this version was seen. */
export function noteDue(release: ReleaseNote, seen: string | undefined): string | undefined {
	if (seen === undefined || seen === release.version) return undefined;
	return release.note ?? undefined;
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
