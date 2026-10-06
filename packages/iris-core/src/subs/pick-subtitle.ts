/**
 * Cross-episode subtitle pick, shared rule with the Android TV client
 * (`SubtitlePick.kt`): keep the two in step when editing either.
 *
 * Per-file progress restores an exact track. Without one, the per-user
 * preferred LANGUAGE decides, and a language usually maps to several
 * tracks in a MULTi release: "Forcés" (forced, and often flagged default
 * by the muxer), "Complets", "SDH". Letting the player rank them, or
 * taking the first match, lands on the forced track: a handful of cues
 * for foreign-language lines, which reads as "subtitles off" to the
 * viewer, on every new episode (Mousetrap S01: ten times over).
 *
 * Ranking within the preferred language: non-forced before forced, and
 * among non-forced the plain track before an SDH/CC one; ties keep
 * source order. No track in the preferred language means no pick: never
 * force a different language onto the viewer.
 */

import { normalizeLang } from '../lang';
import type { SubtitleTrack } from '../manifest-client';

export { normalizeLang };

const SDH_TITLE = /\b(sdh|cc|hearing|malentendant|sourds?)\b/i;

function isSdh(track: SubtitleTrack): boolean {
	return SDH_TITLE.test(track.title ?? '');
}

export function pickPreferredSubtitle(tracks: readonly SubtitleTrack[], preferredLang: string | null | undefined): SubtitleTrack | null {
	const want = normalizeLang(preferredLang);
	if (!want) return null;
	const inLang = tracks.filter((t) => normalizeLang(t.lang) === want);
	if (inLang.length === 0) return null;
	const rank = (t: SubtitleTrack) => (t.forced ? 2 : isSdh(t) ? 1 : 0);
	return inLang.reduce((best, t) => (rank(t) < rank(best) ? t : best));
}
