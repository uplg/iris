// The player's track decisions, framework-free: which renderer a subtitle needs, which audio and
// subtitle a fresh mount starts on, and the names the audio/subtitles panel gives them.

import type { NativeSubtitleTrack } from '@iris/core/engine';
import type { AudioTrack, Manifest, SubtitleTrack } from '@iris/core/manifest-client';
import { nativeSubtitleUrl } from '@iris/core/manifest-client';
import { normalizeLang, pickPreferredSubtitle } from '@iris/core/subs/pick-subtitle';
import { languageName, OFF } from '#lib/language.ts';

export type SubtitleOverlayKind = 'none' | 'native' | 'ass' | 'pgs';

/** WebVTT-able text goes through the engine's `<track>`; ASS and bitmap subs need an overlay. */
export function subtitleOverlayKind(track: SubtitleTrack | null | undefined): SubtitleOverlayKind {
	if (!track) return 'none';
	const codec = track.codec.toLowerCase();
	if (codec === 'ass' || codec === 'ssa') return 'ass';
	if (codec.includes('pgs') || codec.startsWith('hdmv_') || codec.includes('dvb') || codec.includes('dvd_sub')) return 'pgs';
	return 'native';
}

export function classifySubtitles(manifest: Manifest): { native: NativeSubtitleTrack[]; overlay: SubtitleTrack[] } {
	const native: NativeSubtitleTrack[] = [];
	const overlay: SubtitleTrack[] = [];
	for (const t of manifest.subtitles) {
		const kind = subtitleOverlayKind(t);
		if (kind === 'ass' || kind === 'pgs') overlay.push(t);
		else if (kind === 'native') native.push({ ...t, vttUrl: nativeSubtitleUrl(manifest, t.stream_idx) });
	}
	return { native, overlay };
}

/** `?v=<token>` makes the browser (or libass) fetch the extraction again as the torrent grows. */
export function versioned(url: string, version: string | undefined): string {
	return version ? `${url}?v=${encodeURIComponent(version)}` : url;
}

/** Tier F switches audio live through hls.js; every other tier re-spins its pipeline. */
export function tierRequiresRemountForAudio(tier: string): boolean {
	return tier !== 'F';
}

/**
 * The audio a mount starts on: the per-file pick restored from progress (if still valid for
 * this manifest, the track count can change on a re-ingest), else the preferred language
 * (normalised: one release tags `fre`, the next `fra`), else the file's default, else the first.
 */
export function initialAudioIndex(manifest: Manifest, restored: number | undefined, preferredLang: string | null | undefined): number {
	if (typeof restored === 'number' && restored >= 0 && restored < manifest.audio.length) return restored;
	const pref = normalizeLang(preferredLang);
	if (pref) {
		const match = manifest.audio.findIndex((a) => normalizeLang(a.lang) === pref);
		if (match >= 0) return match;
	}
	const flagged = manifest.audio.findIndex((a) => a.default);
	return flagged >= 0 ? flagged : 0;
}

/**
 * The subtitle a mount starts on. `restored === null` is "the person turned them off";
 * `undefined` is no saved pick. Then the preferred language (a language this file lacks means
 * off, never a wrong language; "off" means off), then the file's default, the first native
 * track, the first overlay one (BluRay remuxes that only ship PGS).
 */
export function initialSubtitle(
	manifest: Manifest,
	restored: number | null | undefined,
	preferredLang: string | null | undefined
): SubtitleTrack | null {
	if (restored === null) return null;
	if (typeof restored === 'number') {
		const match = manifest.subtitles.find((s) => s.stream_idx === restored);
		if (match) return match;
	}
	const pref = preferredLang?.trim().toLowerCase();
	if (pref === OFF) return null;
	if (pref) return pickPreferredSubtitle(manifest.subtitles, pref);
	return (
		manifest.subtitles.find((s) => s.default) ??
		manifest.subtitles.find((s) => subtitleOverlayKind(s) === 'native') ??
		manifest.subtitles[0] ??
		null
	);
}

const SDH = /\b(sdh|cc|hi|hearing|malentendants?|sourds?)\b/i;
const DESCRIPTION = /\b(ad|audio ?description|descriptive|described|audiodescription)\b/i;
const SIGNS = /\b(signs?|songs?|forced|forc[ée]s?)\b/i;
const COMMENTARY = /\bcomment(ary|aire)\b/i;
const FRENCH_VARIANT = /\b(vff|vfq|vfi|vf2|vof|vf)\b/i;
const ORIGINAL = /\b(original|vo|vost?)\b/i;

function frenchVariant(title: string | null | undefined): string | null {
	const m = FRENCH_VARIANT.exec(title ?? '');
	return m ? m[1].toUpperCase() : null;
}

function channelsWord(n: number): string | null {
	if (n === 1) return 'mono';
	if (n === 2) return 'stereo';
	if (n === 6) return '5.1';
	if (n === 8) return '7.1';
	return n > 0 ? `${n} channels` : null;
}

/** "English, original", "French (VF)", "English, audio description": unambiguous names. */
function audioBase(a: AudioTrack, index: number): string {
	const lang = languageName(a.lang);
	const title = a.title ?? '';
	const variant = normalizeLang(a.lang) === 'fr' ? frenchVariant(title) : null;
	let name = lang ? (variant ? `${lang} (${variant})` : lang) : title.trim() || `Audio ${index + 1}`;
	if (DESCRIPTION.test(title)) name += ', audio description';
	else if (COMMENTARY.test(title)) name += ', commentary';
	else if (lang && ORIGINAL.test(title)) name += ', original';
	return name;
}

function subtitleBase(s: SubtitleTrack): string {
	const lang = languageName(s.lang);
	const title = s.title ?? '';
	let name = lang ?? (title.trim() || `Subtitles ${s.stream_idx}`);
	if (SDH.test(title)) name += ', for deaf and hard of hearing (SDH)';
	else if (s.forced || SIGNS.test(title)) name += ', signs and songs only';
	return name;
}

/** Names that differ: two tracks still named alike get what tells them apart (their title, then
 * their channels or format, then a number). */
function disambiguate<T>(items: T[], base: (t: T, i: number) => string, extras: ((t: T) => string | null)[]): string[] {
	const out = items.map((t, i) => base(t, i));
	for (const extra of extras) {
		const counts = new Map<string, number>();
		for (const n of out) counts.set(n, (counts.get(n) ?? 0) + 1);
		items.forEach((t, i) => {
			if ((counts.get(out[i]) ?? 0) < 2) return;
			const e = extra(t);
			if (e && !out[i].toLowerCase().includes(e.toLowerCase())) out[i] = `${out[i]}, ${e}`;
		});
	}
	const seen = new Map<string, number>();
	return out.map((n) => {
		const k = (seen.get(n) ?? 0) + 1;
		seen.set(n, k);
		return k > 1 ? `${n} (${k})` : n;
	});
}

export function audioLabels(tracks: AudioTrack[]): string[] {
	return disambiguate(tracks, audioBase, [(a) => a.title?.trim() || null, (a) => channelsWord(a.channels), (a) => a.codec.toUpperCase()]);
}

export function subtitleLabels(tracks: SubtitleTrack[]): string[] {
	return disambiguate(tracks, subtitleBase, [
		(s) => s.title?.trim() || null,
		(s) => (subtitleOverlayKind(s) === 'native' ? 'text' : 'styled')
	]);
}
