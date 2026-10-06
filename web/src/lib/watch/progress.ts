// Where the person is in a file, kept on the server: a heartbeat every 7 s of playback, one on
// pause, one at the end, and a last one (a beacon) when the page goes away. The audio and
// subtitle picks ride along, restored from the saved progress first so the first heartbeat
// never clobbers them with nothing.

import { progress as progressApi, type ProgressBody as Body, type ProgressView } from '@iris/api/client';
import { heartbeatDue, isWatched } from './tier.ts';

export interface ProgressDeps {
	put: (infohash: string, fileIdx: number, body: Body) => Promise<unknown>;
	/** The unload path: `navigator.sendBeacon`, else the client's keepalive save. */
	beacon: (infohash: string, fileIdx: number, body: Body) => void;
}

const browserDeps: ProgressDeps = {
	put: (h, i, b) => progressApi.put(h, i, b),
	beacon: (h, i, b) => {
		if (typeof navigator !== 'undefined' && navigator.sendBeacon) {
			navigator.sendBeacon(progressApi.beaconUrl(h, i), new Blob([JSON.stringify(b)], { type: 'application/json' }));
		} else {
			void progressApi.putOnLeave(h, i, b).catch(() => undefined);
		}
	}
};

export class ProgressSaver {
	lastTime = 0;
	lastSaved = 0;
	duration: number | null = null;
	/** The next save follows a deliberate seek: the server's reset guard lets a near-zero
	 * position replace real progress only then (`put_progress`). */
	seekPending = false;
	audioIdx: number | null = null;
	subtitleIdx: number | null = null;
	#restored = false;

	constructor(
		readonly infohash: string,
		readonly fileIdx: number,
		readonly deps: ProgressDeps = browserDeps
	) {}

	/** Once: the saved picks, so the heartbeats carry them back. */
	restore(p: ProgressView | null | undefined) {
		if (this.#restored) return;
		this.#restored = true;
		if (typeof p?.subtitle_track_idx === 'number') this.subtitleIdx = p.subtitle_track_idx;
		if (typeof p?.audio_track_idx === 'number') this.audioIdx = p.audio_track_idx;
	}

	#consumeSeek() {
		const s = this.seekPending;
		this.seekPending = false;
		return s;
	}

	#send(body: Body) {
		void this.deps.put(this.infohash, this.fileIdx, body).catch(() => undefined);
	}

	timeUpdate(t: number) {
		if (t > 0) this.lastTime = t;
		if (!heartbeatDue(t, this.lastSaved)) return;
		this.lastSaved = t;
		this.#send({
			position_seconds: t,
			duration_seconds: this.duration,
			audio_track_idx: this.audioIdx,
			subtitle_track_idx: this.subtitleIdx,
			completed: isWatched(t, this.duration),
			playing: true,
			seek: this.#consumeSeek()
		});
	}

	durationChange(d: number) {
		if (d > 0) this.duration = d;
	}

	pause(t: number) {
		if (t <= 0) return;
		this.lastSaved = t;
		this.#send({
			position_seconds: t,
			duration_seconds: this.duration,
			audio_track_idx: this.audioIdx,
			subtitle_track_idx: this.subtitleIdx,
			completed: false,
			playing: false,
			seek: this.#consumeSeek()
		});
	}

	ended() {
		this.#send({
			position_seconds: this.duration ?? this.lastTime,
			duration_seconds: this.duration,
			audio_track_idx: this.audioIdx,
			subtitle_track_idx: this.subtitleIdx,
			completed: true
		});
	}

	/** The page goes away: the last position, if it moved since the last save. */
	flush() {
		const t = this.lastTime;
		if (t <= 0 || t === this.lastSaved) return;
		this.lastSaved = t;
		this.deps.beacon(this.infohash, this.fileIdx, {
			position_seconds: t,
			duration_seconds: this.duration,
			audio_track_idx: this.audioIdx,
			subtitle_track_idx: this.subtitleIdx,
			completed: isWatched(t, this.duration),
			seek: this.seekPending
		});
	}
}
