// What a release is doing, decided once for every surface (the library, a title's page, the
// watch page, the polls): whether it still fetches data, its state in one word, when it is done.

import type { TorrentView } from '@iris/api/client';

type Torrent = Pick<TorrentView, 'state' | 'finished' | 'progress_pct' | 'peers' | 'download_speed_bps'> &
	Partial<Pick<TorrentView, 'added_at' | 'fetched_at'>>;

/** How long a fresh grab may sit without peers before it counts as stalled: finding them takes a
 * while (the TV app's STALL_GRACE). Measured between two server stamps, so this device's clock
 * doesn't matter. */
export const STALL_GRACE_MS = 2 * 60_000;

const pastGrace = (t: Torrent): boolean => {
	if (!t.added_at || !t.fetched_at) return true;
	return Date.parse(t.fetched_at) - Date.parse(t.added_at) > STALL_GRACE_MS;
};

/** All its data is on disk. */
export const isComplete = (t: Pick<TorrentView, 'finished' | 'progress_pct'>): boolean => t.finished || t.progress_pct >= 100;

/** Still fetching data: not complete, running or not (a paused or failed one is still short). */
export function isFetching<T extends Pick<TorrentView, 'finished' | 'progress_pct'>>(t: T | undefined): t is T {
	return !!t && !isComplete(t);
}

/** Fetching and running: something actually moves, worth reading again soon. */
export const isMoving = (t: Torrent): boolean => isFetching(t) && (t.state === 'live' || t.state === 'initializing');

export type Phase = 'checking' | 'downloading' | 'stalled' | 'seeding' | 'paused' | 'error';

export function phaseOf(t: Torrent): Phase {
	if (t.state === 'error') return 'error';
	if (t.state === 'paused') return 'paused';
	if (isComplete(t)) return 'seeding';
	if (t.state === 'initializing') return 'checking';
	if (t.peers === 0 && t.download_speed_bps === 0 && pastGrace(t)) return 'stalled';
	return 'downloading';
}

/** A release's state in words (the TV app says the same). */
export const PHASE_WORDS: Record<Phase, string> = {
	checking: 'Checking files',
	downloading: 'Downloading',
	stalled: 'Stalled',
	seeding: 'Seeding',
	paused: 'Paused',
	error: 'Stopped with an error'
};

export const stateWord = (t: Torrent): string => PHASE_WORDS[phaseOf(t)];

/** Seconds until it finishes at its current speed; null when nothing moves. */
export function etaSeconds(t: Pick<TorrentView, 'download_speed_bps' | 'total_size_bytes' | 'progress_bytes'>): number | null {
	if (t.download_speed_bps <= 0) return null;
	return Math.max(0, t.total_size_bytes - t.progress_bytes) / t.download_speed_bps;
}
