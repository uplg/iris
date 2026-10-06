/**
 * Tier A — native `<video src=/stream type=video/mp4>` with HTTP Range.
 *
 * The simplest engine: vanilla `<video>` pointed at the server's raw
 * stream endpoint. The browser handles demux + decode entirely.
 * Used when the source is fMP4 with HW-decodable codecs.
 */

import { appendNativeTrack, bindVideoCallbacks, videoBackedHandle, type EngineHandle, type EngineMount } from '../engine';
import { releaseVideo } from '../mse/media-source';

export const mountTierA: EngineMount = async (opts) => {
	const { container, streamUrl, nativeSubs } = opts;
	container.innerHTML = '';
	const video = document.createElement('video');
	video.className = 'h-full w-full object-contain';
	video.playsInline = true;
	video.preload = 'auto';
	const nativeTrackMap = new Map<number, HTMLTrackElement>();
	for (const sub of nativeSubs) {
		appendNativeTrack(video, sub, nativeTrackMap);
	}
	container.appendChild(video);
	video.src = streamUrl;

	// seek as soon as the duration is known: at `canplay` the element has already buffered
	// from 0 (preload=auto) only to throw it away
	const initialSeek = { done: false, on: 'loadedmetadata' as const };
	const unbind = bindVideoCallbacks(video, opts, initialSeek);

	let firstPlayable = false;
	const onLoadedData = () => {
		if (firstPlayable) return;
		firstPlayable = true;
		opts.onReady?.();
	};
	video.addEventListener('loadeddata', onLoadedData);

	// One-shot. See the comment in `tier-f-hls.ts` for rationale.
	let errorFired = false;
	const onErr = () => {
		if (errorFired) return;
		errorFired = true;
		const err = video.error;
		const msg = err ? `media error ${err.code}: ${err.message}` : 'video element error';
		opts.onError(new Error(msg));
	};
	video.addEventListener('error', onErr);

	const handle: EngineHandle = videoBackedHandle(video, {
		nativeTrackMap,
		fallbackDuration: opts.manifest.duration_s ?? null,
		dispose: async () => {
			unbind();
			video.removeEventListener('loadeddata', onLoadedData);
			video.removeEventListener('error', onErr);
			releaseVideo(video);
		}
	});
	return handle;
};
