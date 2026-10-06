// An episode row's state in words, with a tone (never color alone): what the person can do
// with it now, from what is on disk, how far they watched, and what the indexer offers.

import type { TorrentView } from '@iris/api/client';
import { duration, percent, plural, timeLeft } from '@iris/api/format';
import type { Tone } from '#lib/components/StatusLine.svelte';
import { etaSeconds, isFetching } from '#lib/torrent.ts';
import { watchedShare } from '#lib/watched.ts';
import { languageWord, listWords, type Available, type Downloaded, type Episode, type Gone } from './merge.ts';

/** What the row's first downloaded release does when pressed. */
export type Verb = 'Resume' | 'Play' | 'Watch again' | 'Play while downloading';

export interface RowState {
	tone: Tone;
	text: string;
	/** Watched so far, 0 to 1, when in progress. */
	progress?: number;
	verb?: Verb;
}

export interface Lookup {
	torrent: (infohash: string) => TorrentView | undefined;
}

/** « done in about 6 min », or why it is not moving. */
export function eta(t: TorrentView): string {
	if (t.state === 'paused') return 'paused';
	if (t.state === 'error') return t.error ? `stopped with an error: ${t.error}` : 'stopped with an error';
	const left = etaSeconds(t);
	if (left === null) return t.peers > 0 ? 'starting' : 'waiting for peers';
	return `done in about ${duration(left)}`;
}

export function rowState(ep: Episode, look: Lookup): RowState {
	const disk = ep.variants.filter((v): v is Downloaded => v.status === 'downloaded');
	const offers = ep.variants.filter((v): v is Available => v.status === 'available');
	const gone = ep.variants.filter((v): v is Gone => v.status === 'gone');

	if (disk.length) {
		const first = disk[0];
		const t = look.torrent(first.infohash);
		const length = first.duration_seconds && first.duration_seconds > 0 ? first.duration_seconds : null;
		const at = first.position_seconds ?? 0;
		if (isFetching(t)) {
			return {
				tone: t.state === 'error' ? 'warn' : 'busy',
				text: `Downloading · ${percent(t.progress_pct)} · ${eta(t)}`,
				verb: 'Play while downloading'
			};
		}
		if (first.watched) return { tone: 'ok', text: length ? `Watched · ${duration(length)}` : 'Watched', verb: 'Watch again' };
		if (at > 0) {
			const left = length ? ` · ${timeLeft(length - at)}` : '';
			return { tone: 'info', text: `In progress${left}`, progress: watchedShare(at, length) ?? undefined, verb: 'Resume' };
		}
		return { tone: 'ok', text: length ? `On disk · ${duration(length)}` : 'On disk', verb: 'Play' };
	}
	if (gone.length) {
		const watched = gone.some((g) => g.watched);
		return { tone: 'info', text: watched ? 'Watched · removed from disk to free space' : 'Removed from disk to free space' };
	}
	if (offers.length) {
		const langs = offers.map((o) => languageWord(o.language)).filter((w): w is string => !!w);
		const said = langs.length ? ` · ${listWords(langs)} audio` : '';
		return { tone: 'available', text: `Available · ${plural(offers.length, 'release')}${said}` };
	}
	return { tone: 'info', text: 'Not available yet' };
}

/** Offers grouped by language: one « Grab and play » per language (the server picks the best
 * release in that slot). */
export function offersByLanguage(ep: Episode): Available[][] {
	const by = new Map<string, Available[]>();
	for (const v of ep.variants) {
		if (v.status !== 'available') continue;
		const key = v.language ?? '';
		by.set(key, [...(by.get(key) ?? []), v]);
	}
	return [...by.values()];
}
