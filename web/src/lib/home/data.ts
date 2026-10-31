// The words the home and discover screens say about what they read: the "Right now" facts,
// the downloads per collection, a title's kind, said once.

import type { ContinueWatchingItem, FriendActivity, FriendEvent, HomeSummary, ReleaseNote, TorrentView } from '@iris/api/client';
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

const FRIENDS_SHOWN = 3;
const SHOWN_PER_FRIEND = 3;

/** One thing a friend did; no `verb` when it repeats the one before (« watched A, B »). */
export interface FriendItem {
	key: string;
	verb: string | null;
	title: string;
	collection_id: string | null | undefined;
}

/** One friend's line of the home: their latest news, and when, unless they are playing it now. */
export interface FriendRow {
	friend_id: string;
	name: string;
	at: string | null;
	items: FriendItem[];
}

/** What friends do, a line each, who plays now first: « Muriel added Dune, watched The Bear · S3:E1 ». */
export function friendRows(activity: FriendActivity): FriendRow[] {
	const byFriend = new Map<string, FriendEvent[]>();
	for (const e of [...activity.live, ...activity.recent]) byFriend.set(e.friend_id, [...(byFriend.get(e.friend_id) ?? []), e]);
	return [...byFriend.values()].slice(0, FRIENDS_SHOWN).map((events) => {
		const shown = events.slice(0, SHOWN_PER_FRIEND);
		const [latest] = shown;
		return {
			friend_id: latest.friend_id,
			name: latest.friend_name,
			at: latest.action === 'watching' ? null : latest.at,
			items: shown.map((e, i) => {
				const code = episodeCode(e.season, e.episode);
				return {
					key: `${e.action}:${e.collection_id ?? e.title}`,
					verb: shown[i - 1]?.action === e.action ? null : DOES[e.action],
					title: `${e.title}${code ? ` · ${code}` : ''}`,
					collection_id: e.collection_id
				};
			})
		};
	});
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
