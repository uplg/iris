/**
 * Tier C / D — WebCodecs decode + canvas render + Web Audio.
 *
 * "Bypass MSE entirely": Mediabunny demux → `VideoDecoder` /
 * `AudioDecoder` → renderer (Canvas2D today, WebGPU when available).
 * The audio scheduler is the master clock (a wall clock when the file has
 * no audio); the renderer chases it, and both decoders are paced on it:
 * they decode a short lead ahead of the clock, never the file flat out.
 *
 * Implements full `EngineHandle`:
 *   - `play` / `pause` — resume / suspend the clock: the AudioContext stops
 *     consuming, so the renderer and the decode pacing park with it.
 *   - `seek` — drain decoders, reset the clock, re-spin pipelines from a new
 *     keyframe. Frame-accurate to the nearest key packet.
 *   - `setVolume` / `setMuted` — scheduler `GainNode`.
 *   - `onEnded` — when the clock reaches the end, not when the decoder does.
 */

import { ALL_FORMATS, Input, type InputAudioTrack, type InputVideoTrack } from 'mediabunny';

import { isHevc } from '../codec';
import { startAudioPipeline, type AudioPipelineHandle } from '../decode/audio-pipeline';
import { AUDIO_LEAD_S, MAX_QUEUED_FRAMES, playbackEnded, VIDEO_LEAD_S, withinLead, type EndState } from '../decode/pacing';
import { startVideoPipeline, type VideoPipelineHandle } from '../decode/video-pipeline';
import { probeVideoTrack } from '../decode/webcodecs-probe';
import { createAudioScheduler, type AudioScheduler } from '../audio/audio-scheduler';
import { WallClock } from '../audio/wall-clock';
import { irisUrlSource, VOD_RETRY } from '../stream-source';
import { mountRenderer, type VideoRenderer } from '../render/renderer-factory';
import { defaultAudioIndex, manifestAudioTracks, type EngineAudioTrack, type EngineHandle, type EngineMount } from '../engine';

export const mountTierC: EngineMount = async (opts) => {
	const { container, streamUrl, startPosition, audioTrackIndex } = opts;
	const onError = opts.onError;

	container.innerHTML = '';

	const input = new Input({
		source: irisUrlSource(streamUrl, { cacheBytes: 16 * 1024 * 1024, ...VOD_RETRY }),
		formats: ALL_FORMATS
	});
	const disposeInput = async () => {
		try {
			await input.dispose();
		} catch {
			/* idempotent */
		}
	};

	// any step that throws (a read error, the AudioContext refused) disposes the input and its
	// read cache before the mount fails
	const open = async () => {
		const videoTrack = (await input.getPrimaryVideoTrack()) ?? null;
		if (!videoTrack) throw new Error('Tier C: no primary video track');
		const probe = await probeVideoTrack(videoTrack);
		if (!probe || !probe.decodes) throw new Error('Tier C probe: decoder rejected the keyframe');

		// Pick the audio track. The chrome's audio picker writes
		// `audioTrackIndex` (= position in `manifest.audio`); we walk
		// Mediabunny's input.getAudioTracks() and take that index.
		// Defaults to the primary track when no index is supplied.
		const allAudio = await input.getAudioTracks();
		const audioTrack =
			audioTrackIndex !== null && audioTrackIndex !== undefined && audioTrackIndex >= 0 && audioTrackIndex < allAudio.length
				? (allAudio[audioTrackIndex] ?? null)
				: ((await input.getPrimaryAudioTrack()) ?? null);
		const audioConfig = audioTrack ? await audioTrack.getDecoderConfig() : null;
		const scheduler: AudioScheduler = await createAudioScheduler({ sampleRate: audioConfig?.sampleRate });
		return { videoTrack, probe, audioTrack, audioConfig, scheduler };
	};
	const { videoTrack, probe, audioTrack, audioConfig, scheduler } = await open().catch(async (e: unknown) => {
		await disposeInput();
		throw e;
	});
	const hasAudio = !!(audioTrack && audioConfig);
	const wall = new WallClock(() => performance.now());

	let currentSeekTarget = startPosition;
	/** Where playback is: the master clock once it is anchored (the first audio buffer, or the
	 *  first frame without audio), the seek target until then — never the 0 an unanchored
	 *  clock reads, which flashed the scrubber to the start and made a relative seek land
	 *  near 0:00. */
	const currentMediaTime = (): number => {
		if (hasAudio) return scheduler.isAnchored() ? scheduler.currentMediaTimeSeconds() : currentSeekTarget;
		return wall.anchored ? wall.time() : currentSeekTarget;
	};

	let renderer: VideoRenderer;
	try {
		renderer = await mountRenderer({
			container,
			clockSeconds: currentMediaTime,
			hdr: isHevc(probe.config.codec) ? 'auto' : 'sdr',
			onError
		});
	} catch (e) {
		await scheduler.dispose();
		await disposeInput();
		throw e;
	}

	let readyFired = false;
	const fireReady = () => {
		if (readyFired) return;
		readyFired = true;
		opts.onReady?.();
	};

	// Pipeline state that survives across seeks.
	let videoHandle: VideoPipelineHandle | null = null;
	let audioHandle: AudioPipelineHandle | null = null;
	let seekGeneration = 0;
	let paused = false;
	let disposed = false;
	let endedFired = false;
	const end: EndState = { videoDone: false, lastVideoTs: null, hasAudio, audioDone: false, audioEndTs: null };
	// Debug-panel counters. Cheap: two increments on paths that already run
	// per frame and per audio buffer.
	let framesRendered = 0;
	let audioBuffers = 0;

	const spinPipelines = (fromSeconds: number, generation: number): void => {
		end.videoDone = false;
		end.lastVideoTs = null;
		end.audioDone = false;
		end.audioEndTs = null;
		endedFired = false;
		videoHandle = startVideoPipeline({
			track: videoTrack as InputVideoTrack,
			config: probe.config,
			startSeconds: fromSeconds,
			canDecode: (ts) => withinLead(ts, currentMediaTime(), VIDEO_LEAD_S) && renderer.queueDepth() < MAX_QUEUED_FRAMES,
			onFrame: (frame) => {
				if (generation !== seekGeneration || disposed) {
					frame.close();
					return;
				}
				const ts = frame.timestamp / 1_000_000;
				if (end.lastVideoTs === null || ts > end.lastVideoTs) end.lastVideoTs = ts;
				// without audio the first frame starts the clock, paused or not
				if (!hasAudio && !wall.anchored) wall.anchor(Math.max(ts, currentSeekTarget));
				renderer.enqueue(frame);
				framesRendered += 1;
				fireReady();
			},
			onError,
			onEnd: () => {
				if (generation === seekGeneration) end.videoDone = true;
			}
		});
		if (audioTrack && audioConfig) {
			audioHandle = startAudioPipeline({
				track: audioTrack as InputAudioTrack,
				config: audioConfig,
				startSeconds: fromSeconds,
				canDecode: (ts) => withinLead(ts, currentMediaTime(), AUDIO_LEAD_S),
				onData: (data) => {
					if (generation !== seekGeneration || disposed) {
						data.close();
						return;
					}
					const endTs = (data.timestamp + data.duration) / 1_000_000;
					if (end.audioEndTs === null || endTs > end.audioEndTs) end.audioEndTs = endTs;
					scheduler.enqueue(data);
					audioBuffers += 1;
				},
				onError,
				onEnd: () => {
					if (generation === seekGeneration) end.audioDone = true;
				}
			});
		}
	};

	spinPipelines(startPosition, seekGeneration);

	// 4 Hz time-update broadcast so the parent can save resume position
	// and the chrome can update its display; the end is noticed here too.
	const tickInterval = setInterval(() => {
		const t = currentMediaTime();
		opts.onTimeUpdate?.(t);
		if (!endedFired && !disposed && playbackEnded(end, t)) {
			endedFired = true;
			opts.onEnded?.();
		}
	}, 250);

	const handle: EngineHandle = {
		dispose: async () => {
			disposed = true;
			clearInterval(tickInterval);
			await Promise.allSettled([videoHandle?.stop() ?? Promise.resolve(), audioHandle?.stop() ?? Promise.resolve()]);
			renderer.dispose();
			await scheduler.dispose();
			await disposeInput();
		},
		stats: () => [
			['time', `${currentMediaTime().toFixed(2)} / ${opts.manifest.duration_s?.toFixed(1) ?? '?'}`],
			['state', paused ? 'paused' : 'playing'],
			['render', `canvas, ${framesRendered} frame(s) enqueued, ${renderer.queueDepth()} waiting`],
			['decode', `${probe.config.codec ?? '?'} via WebCodecs${probe.hardware ? ', hardware' : ', software'}`],
			['audio', hasAudio ? `${audioBuffers} buffer(s), clock at ${scheduler.currentMediaTimeSeconds().toFixed(2)}s` : 'none, wall clock'],
			['pipeline', `seek generation ${seekGeneration}, target ${currentSeekTarget.toFixed(1)}s`]
		],
		currentTime: () => currentMediaTime(),
		duration: () => opts.manifest.duration_s ?? null,
		paused: () => paused,
		volume: () => scheduler.getVolume(),
		muted: () => scheduler.getMuted(),
		buffered: () => [],
		play: async () => {
			// resume even when already playing: an AudioContext created without a user
			// gesture starts suspended, and this is the gesture
			scheduler.resume();
			wall.resume();
			if (!paused) return;
			paused = false;
			renderer.setPaused(false);
			opts.onPlayingChange?.(true);
		},
		pause: () => {
			if (paused) return;
			paused = true;
			// the clock freezes; the renderer and the decode pacing park with it
			scheduler.suspend();
			wall.pause();
			renderer.setPaused(true);
			opts.onPlayingChange?.(false);
			opts.onPause?.(currentMediaTime());
		},
		seek: (seconds: number) => {
			// Tier C seek: drain → reset scheduler → re-spin pipelines from
			// the closest preceding keyframe. Frame-accurate to the nearest
			// key packet. The bumped generation ID makes any in-flight
			// decode output from the previous run get dropped on arrival.
			const target = Math.max(0, seconds);
			seekGeneration += 1;
			currentSeekTarget = target;
			const gen = seekGeneration;
			const prevVideo = videoHandle;
			const prevAudio = audioHandle;
			videoHandle = null;
			audioHandle = null;
			// the old timeline goes now: queued frames would hold the new run's pacing back
			// (a backward seek leaves them all "early"), and the clock reads the target again
			scheduler.resetClock();
			wall.reset();
			renderer.clear();
			void (async () => {
				await Promise.allSettled([prevVideo?.stop() ?? Promise.resolve(), prevAudio?.stop() ?? Promise.resolve()]);
				if (disposed || gen !== seekGeneration) return;
				opts.onSeeking?.(target);
				spinPipelines(target, gen);
			})();
		},
		setVolume: (v) => scheduler.setVolume(v),
		setMuted: (m) => scheduler.setMuted(m),
		audioTracks: (): EngineAudioTrack[] => manifestAudioTracks(opts.manifest, audioTrackIndex ?? defaultAudioIndex(opts.manifest)),
		// Tier C audio switch needs a remount (the decoder is bound to a
		// single Mediabunny track). `IrisPlayer` triggers that via the
		// mount-key including `audioTrackIndex`.
		setAudioTrack: () => undefined,
		setNativeSubtitle: () => undefined,
		videoElement: () => null,
		canvasElement: () => renderer.canvas
	};
	return handle;
};
