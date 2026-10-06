/**
 * Media Session API wiring. Surfaces playback metadata + transport
 * controls to the OS (lock screen, media keys, AirPods/Bluetooth
 * remote-press, Linux MPRIS).
 *
 * Tier-agnostic: routes everything through `EngineHandle`, so canvas-
 * only Tier C still gets OS integration that the legacy `<video>` PiP
 * couldn't provide.
 */

import type { EngineHandle } from '../engine';
import type { Manifest } from '../manifest-client';

export type MediaSessionWiring = {
	/** Pushes the play state and the position to the OS. Called on what changes them (play,
	 *  pause, seek, duration, rate): the OS extrapolates the position in between. */
	sync: () => void;
	dispose: () => void;
};

export function attachMediaSession(
	handle: EngineHandle,
	manifest: Manifest,
	meta: { title: string; artwork?: MediaImage[] }
): MediaSessionWiring {
	if (typeof navigator === 'undefined' || !('mediaSession' in navigator)) {
		return { sync: () => undefined, dispose: () => undefined };
	}
	const ms = navigator.mediaSession;

	ms.metadata = new MediaMetadata({
		title: meta.title,
		artist: manifest.filename,
		album: 'Iris',
		artwork: meta.artwork ?? []
	});

	const actions: Array<[MediaSessionAction, MediaSessionActionHandler]> = [
		['play', () => void handle.play()],
		['pause', () => handle.pause()],
		[
			'seekto',
			(e) => {
				if (typeof e.seekTime === 'number') handle.seek(e.seekTime);
			}
		],
		[
			'seekbackward',
			(e) => {
				const step = typeof e.seekOffset === 'number' ? e.seekOffset : 10;
				handle.seek(Math.max(0, handle.currentTime() - step));
			}
		],
		[
			'seekforward',
			(e) => {
				const step = typeof e.seekOffset === 'number' ? e.seekOffset : 10;
				handle.seek(handle.currentTime() + step);
			}
		],
		['stop', () => handle.pause()]
	];
	for (const [action, fn] of actions) {
		try {
			ms.setActionHandler(action, fn);
		} catch {
			// Some actions are platform-gated (e.g., `seekto` requires the
			// browser to know duration). Silently skip unsupported ones.
		}
	}

	// the element's own events, for the engines that have one; the canvas engines' callbacks
	// reach `sync` through the player
	const video = handle.videoElement();
	// setPositionState was added incrementally; guard the call.
	const sync = () => {
		try {
			ms.playbackState = handle.paused() ? 'paused' : 'playing';
		} catch {
			/* torn down */
		}
		const duration = handle.duration();
		if (duration === null || duration === undefined || !Number.isFinite(duration) || duration <= 0) return;
		try {
			ms.setPositionState({
				duration,
				playbackRate: video?.playbackRate || 1,
				position: Math.max(0, Math.min(duration, handle.currentTime()))
			});
		} catch {
			/* unsupported: noop */
		}
	};
	const EVENTS = ['play', 'pause', 'playing', 'seeked', 'ratechange', 'durationchange', 'loadedmetadata'] as const;
	for (const e of EVENTS) video?.addEventListener(e, sync);
	sync();

	return {
		sync,
		dispose: () => {
			for (const e of EVENTS) video?.removeEventListener(e, sync);
			try {
				ms.metadata = null;
				for (const [action] of actions) ms.setActionHandler(action, null);
				ms.playbackState = 'none';
			} catch {
				/* idempotent */
			}
		}
	};
}
