// What the chrome shows of the engine, as fine-grained runes, refreshed by events only: the
// engine's own callbacks (time, duration, playing, pause), the media element's events when the
// engine has one (progress, volumechange, ratechange…), and a read after each command the
// chrome sends. No animation-frame polling: one field changes, one text node updates.

import type { EngineHandle } from '@iris/core/engine';

type Range = [number, number];

const sameRanges = (a: Range[], b: Range[]) => a.length === b.length && a.every((r, i) => r[0] === b[i][0] && r[1] === b[i][1]);

/** Element events that can change what the chrome shows. */
const VIDEO_EVENTS = [
	'progress',
	'volumechange',
	'ratechange',
	'durationchange',
	'play',
	'pause',
	'playing',
	'seeked',
	'emptied',
	'loadedmetadata'
] as const;

export class MediaState {
	time = $state(0);
	duration = $state<number | null>(null);
	paused = $state(true);
	volume = $state(1);
	muted = $state(false);
	rate = $state(1);
	buffered = $state<Range[]>([]);
	/** The engine makes the person wait (buffering, seeking, a Tier E fragment transcoding). */
	busy = $state(true);

	constructor(duration: number | null = null) {
		this.duration = duration;
	}

	/** Reads everything the handle reports (after a command, on an engine event). */
	read(h: EngineHandle | null) {
		if (!h) return;
		try {
			const t = h.currentTime();
			if (Number.isFinite(t)) this.time = t;
			const d = h.duration();
			if (d !== null && Number.isFinite(d) && d > 0) this.duration = d;
			this.paused = h.paused();
			this.volume = h.volume();
			this.muted = h.muted();
			const b = h.buffered();
			if (!sameRanges(b, this.buffered)) this.buffered = b;
			const v = h.videoElement();
			if (v) this.rate = v.playbackRate;
		} catch {
			// an engine mid-teardown: keep what was shown
		}
	}

	/** Follows the media element's events while the handle lives; returns the detach. */
	follow(h: EngineHandle): () => void {
		const v = h.videoElement();
		this.read(h);
		if (!v) return () => undefined;
		const onEvent = () => this.read(h);
		for (const e of VIDEO_EVENTS) v.addEventListener(e, onEvent);
		return () => {
			for (const e of VIDEO_EVENTS) v.removeEventListener(e, onEvent);
		};
	}
}
