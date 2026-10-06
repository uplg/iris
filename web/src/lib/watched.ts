// Where the person is in a title, from the server's `watch` (library cards, the search's
// library matches): one reading for every surface.

import type { TitleWatch } from '@iris/api/client';
import { episodeCode, timeLeft } from '@iris/api/format';

/** A position past the opening seconds, not finished: worth resuming. */
const RESUMABLE_SECONDS = 5;

export interface Resume {
	infohash: string;
	fileIdx: number;
	/** `S2:E4`, or null for a movie. */
	code: string | null;
	/** Seconds left, when the length is known. */
	left: number | null;
	/** Watched so far, 0 to 1, when the length is known. */
	share: number | null;
}

/** The file to resume, when the last one watched was left mid-way. */
export function resumeOf(w: TitleWatch | null | undefined): Resume | null {
	if (!w || w.completed || w.position_seconds < RESUMABLE_SECONDS) return null;
	const length = typeof w.duration_seconds === 'number' && w.duration_seconds > 0 ? w.duration_seconds : null;
	return {
		infohash: w.infohash,
		fileIdx: w.file_idx,
		code: episodeCode(w.season, w.episode),
		left: length === null ? null : Math.max(0, length - w.position_seconds),
		share: length === null ? null : Math.min(1, w.position_seconds / length)
	};
}

/** Every episode on disk finished (a movie: its file finished). */
export function allWatched(w: TitleWatch | null | undefined, kind: string, episodeCount: number): boolean {
	if (!w) return false;
	return kind === 'tv' ? episodeCount > 0 && w.watched_episodes >= episodeCount : w.completed;
}

/** « In progress · S1:E7 · 23 min left », « Watched », « Last watched S1:E7 », or null. */
export function watchWords(w: TitleWatch | null | undefined, kind: string, episodeCount: number): string | null {
	if (!w) return null;
	if (allWatched(w, kind, episodeCount)) return 'Watched';
	const r = resumeOf(w);
	if (r) return ['In progress', r.code, r.left === null ? null : timeLeft(r.left)].filter(Boolean).join(' · ');
	const code = episodeCode(w.season, w.episode);
	return code ? `Last watched ${code}` : null;
}
