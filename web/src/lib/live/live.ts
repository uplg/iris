// Live TV decisions, framework-free: where now is inside a programme, the engine for the
// elected source, the stand-in manifest of an endless stream, and when a failing feed is
// retried, rotated or given up.

import type { LiveProgramme } from '@iris/api/client';
import type { DecodeTier, Manifest } from '@iris/core/manifest-client';

/** 0–100 position of `now` inside a programme, null outside its window. */
export function programmeProgress(startIso: string, stopIso: string, now = Date.now()): number | null {
	const start = Date.parse(startIso);
	const stop = Date.parse(stopIso);
	if (!Number.isFinite(start) || !Number.isFinite(stop) || stop <= start) return null;
	const pos = ((now - start) / (stop - start)) * 100;
	if (pos < 0 || pos > 100) return null;
	return Math.round(pos);
}

/** A clock time the viewer's way (`21:05`); '' for a bad date. */
export function clockTime(iso: string): string {
	const d = new Date(iso);
	return Number.isNaN(d.getTime()) ? '' : d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

/** « Now: News, until 21:00 » / « Next at 21:00: Film », for a tile and the watch page. */
export function nowWords(p: LiveProgramme): string {
	const until = clockTime(p.stop);
	return until ? `Now: ${p.title}, until ${until}` : `Now: ${p.title}`;
}
export function nextWords(p: LiveProgramme): string {
	const at = clockTime(p.start);
	return at ? `Next at ${at}: ${p.title}` : `Next: ${p.title}`;
}

/**
 * The engine for the source the backend elected (`x-iris-live-upstream`): the tuner's feed
 * (broadcast -c copy, open-GOP H.264 + E-AC-3) plays through the WebCodecs live engine with
 * its own concealment (MSE kills the pipeline on mid-stream joins; hls.js rejects `ec-3` in a
 * muxed fMP4); without WebCodecs (iOS Safari) Tier F, the native HLS pipeline decoding E-AC-3
 * on Apple hardware. Every other feed: Tier F live (hls.js, + the E-AC-3 WebAudio sidecar).
 */
export function liveTier(upstream: string | null, webcodecs: boolean): DecodeTier {
	return upstream === 'tuner' && webcodecs ? 'C' : 'F';
}

/** Floor for automatic source rotations; the real budget is the channel's source count
 * (`x-iris-live-sources`: M6 is carried by four feeds, a budget of two never reaches the fourth). */
export const MIN_AUTO_ROTATIONS = 2;

export function sourceCount(header: string | null): number {
	const n = Number(header);
	return Number.isFinite(n) && n > 0 ? n : MIN_AUTO_ROTATIONS;
}

/**
 * Failure policy in two stages. The first failure remounts the same source silently (a 401, a
 * join timeout is no evidence the source is bad, and reporting it sends the tuner into a
 * cooldown that strands the next probes: the « dead until a forced refresh » spiral). A repeat
 * failure reports the source and rotates, within the budget; past it, the viewer is told.
 */
export class LiveRotation {
	softRetried = false;
	rotations = 0;

	next(sources: number): 'retry' | 'rotate' | 'give-up' {
		if (!this.softRetried) {
			this.softRetried = true;
			return 'retry';
		}
		this.softRetried = false;
		if (this.rotations < Math.max(MIN_AUTO_ROTATIONS, sources)) {
			this.rotations += 1;
			return 'rotate';
		}
		return 'give-up';
	}

	/** « Retry » after giving up: a fresh budget. */
	reset() {
		this.rotations = 0;
	}
}

/** The player is manifest-driven; a live stream has no probed tracks (the engines read the
 * codecs from the playlists): empty lists keep the audio/subtitles button away, a null
 * duration makes it endless. */
export function liveManifest(channelName: string): Manifest {
	return {
		schema_version: 1,
		infohash: 'live',
		file_idx: 0,
		filename: channelName,
		container: 'hls',
		size_bytes: 0,
		duration_s: null,
		download: { bytes_complete: 0, progress: 1, ranges_complete: [] },
		header_byte_range: { start: 0, end: 0 },
		index_at_end: false,
		moov_at_start: null,
		tail_byte_range: null,
		video: [],
		audio: [],
		subtitles: [],
		chapters: []
	};
}
