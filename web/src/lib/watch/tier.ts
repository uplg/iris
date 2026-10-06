// The watch page's playback decisions, framework-free (unit-tested in node): which tier after
// a failure, where the stream comes from, when it is ready, what to resume from.

import { hlsUrl, rawStreamUrl, type DecodeTier, type Manifest } from '@iris/core/manifest-client';
import { isHevc } from '@iris/core/codec';
import type { PlayStatus, ProgressView, TorrentView } from '@iris/api/client';
import { isResumable } from '#lib/watched.ts';

/** `?tier=F` (A to F) pins the engine: a debug override for one code path. */
export function forcedTier(search: string): DecodeTier | null {
	const forced = new URLSearchParams(search).get('tier');
	return forced && /^[A-F]$/i.test(forced) ? (forced.toUpperCase() as DecodeTier) : null;
}

/** A tier already demoted away from on this mount collapses straight to F. */
export function afterDemotions(picked: DecodeTier, demoted: ReadonlySet<DecodeTier>): DecodeTier {
	return demoted.has(picked) ? 'F' : picked;
}

export interface DemotionEnv {
	userAgent: string;
	/** Gecko 154+ on macOS: HEVC only opens on an IDR (caps.hevcMseNeedsIdrStart). */
	hevcNeedsIdrStart: boolean;
	demoted: ReadonlySet<DecodeTier>;
}

/**
 * The tier after a failure. C/D failing on HEVC in a desktop Chromium at ≤ 1080p goes through
 * E (hevc.js) before the server's HLS; a Gecko-on-macOS Tier B (the CRA splice) refusing the
 * stream goes to E too, which can still seek, not to F whose fragments meet the same demuxer.
 */
export function nextDemotionTarget(from: DecodeTier, manifest: Manifest | undefined, env: DemotionEnv): DecodeTier {
	const primary = manifest?.video[0];
	const hevc = isHevc(primary?.codec);
	if ((from === 'C' || from === 'D') && manifest) {
		const h = primary?.height ?? 0;
		const chromiumish = /Chrome|Edg/.test(env.userAgent) && !/Mobile/.test(env.userAgent);
		if (hevc && h > 0 && h <= 1080 && chromiumish && !env.demoted.has('E')) return 'E';
	}
	if (from === 'B' && hevc && env.hevcNeedsIdrStart && !env.demoted.has('E')) return 'E';
	return 'F';
}

/** What a genuine engine error does (the backend reachable): A and F have nowhere to go, the
 * error is said; the others demote. */
export function errorOutcome(from: DecodeTier): 'say' | 'demote' {
	return from === 'A' || from === 'F' ? 'say' : 'demote';
}

/**
 * Tracks the demotions of one mount. A tier is demoted once: the dying engine's follow-on
 * errors (fired before its dispose unwinds) find it already demoted and are swallowed. (The
 * React page also held an « in progress » flag cleared by a 250 ms timer; it guarded the same
 * case the set already does, so it is not carried over.)
 */
export class Demotions {
	readonly demoted = new Set<DecodeTier>();

	/** The tier to switch to, or null when `from` was already demoted. */
	demote(from: DecodeTier, target: (from: DecodeTier) => DecodeTier): DecodeTier | null {
		if (this.demoted.has(from)) return null;
		const to = target(from);
		this.demoted.add(from);
		return to;
	}

	reset() {
		this.demoted.clear();
	}
}

/** The stream for a tier: the server's HLS for F, the raw bytes otherwise. `nonce` (after a
 * backend outage) changes the URL so the player remounts on the same tier; the backend ignores
 * the parameter. */
export function playSource(tier: DecodeTier, infohash: string, fileIdx: number, nonce: number): { src: string; type: string } {
	const base = tier === 'F' ? hlsUrl(infohash, fileIdx) : rawStreamUrl(infohash, fileIdx);
	const src = nonce > 0 ? `${base}${base.includes('?') ? '&' : '?'}_r=${nonce}` : base;
	return { src, type: tier === 'F' ? 'application/vnd.apple.mpegurl' : 'video/mp4' };
}

/** Only Tier F waits for the server's remux (/play/status); the others play once the manifest is in. */
export function sourceReady(tier: DecodeTier, playReady: boolean, hasProbe: boolean, hasManifest: boolean): boolean {
	return tier === 'F' ? playReady && hasProbe : hasManifest;
}

/** The play-status poll: every second until the cache is ready or prep failed for good
 * (including while the request itself fails: Tier F is gated on it). */
export function playStatusInterval(d: PlayStatus | undefined): number | false {
	return d?.ready || d?.error ? false : 1000;
}

/** The subtitle cache-buster: 5 % download buckets (at most ~20 re-fetches over a fast
 * download), `final` once finished so the response can be cached for good. */
export function subtitleVersion(t: TorrentView | undefined): string {
	if (!t) return '0';
	if (t.finished) return 'final';
	return Math.floor(t.progress_pct / 5).toString();
}

/** Where playback starts: the saved position once resumable, unless the file was finished. */
export function resumeFrom(p: ProgressView | null | undefined): number {
	if (!p || p.completed) return 0;
	return isResumable(p.position_seconds) ? p.position_seconds : 0;
}

/** Watched once past 90 % (the credits), for movies and episodes alike (the TV player's rule). */
export const WATCHED_FRACTION = 0.9;
/** The control bar offers the next episode past 95 %. */
export const NEXT_EPISODE_FRACTION = 0.95;

export const isWatched = (t: number, dur: number | null): boolean => dur !== null && dur > 0 && t >= dur * WATCHED_FRACTION;
export const isNearEnd = (t: number, dur: number | null): boolean => dur !== null && dur > 0 && t / dur >= NEXT_EPISODE_FRACTION;

/** A position worth saving: past the opening seconds, and 7 s away from the last save, either way (a seek back counts). */
export const heartbeatDue = (t: number, lastSaved: number): boolean => isResumable(t) && Math.abs(t - lastSaved) > 7;

/** The probe and manifest polls: the « not on disk yet » answer means try again. */
export const notOnDisk = (e: unknown): boolean => e instanceof Error && e.message.includes('not yet on disk');
