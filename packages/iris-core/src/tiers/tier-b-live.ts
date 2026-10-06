/**
 * Tier B live — full-mediabunny live HLS playback → MSE. One engine for
 * EVERY MSE browser (Chrome, Firefox, desktop Safari alike).
 *
 * The engine for the household tuner's remuxed feed (fMP4 HLS, H.264
 * 1080i video + broadcast E-AC-3/AC-3 audio). hls.js cannot play it:
 * a muxed fMP4 becomes ONE `audiovideo` SourceBuffer whose codec
 * string includes `ec-3`, which Chrome/Firefox reject wholesale —
 * video included. So the browser mirrors the VOD Tier B pipeline:
 *
 *   master.m3u8 → mediabunny Input (HLS live: follows playlist
 *     refreshes until ENDLIST)
 *     → video: EncodedPacketSink passthrough (no decode)
 *     → audio: AudioSampleSink (libav.js decodes E-AC-3 → PCM)
 *              → AudioSampleSource (WebCodecs AudioEncoder → AAC/Opus)
 *     → mediabunny Output (fragmented MP4) → SourceBuffer
 *
 * Timeline: `offsetTimestampsByDateTime: false` (gapless, built from
 * segment durations) and both tracks re-timestamped to `ts - anchor`
 * (anchor = first fed keyframe). One shift on both sides keeps A/V
 * sync by construction, keeps the timeline at ~0 for MSE, and keeps
 * epoch-scale values out of the WebCodecs encoders — Firefox's Opus
 * AudioEncoder silently produces nothing at ~1.8e15 µs timestamps.
 * The live-edge anchor comes from the PLAYLIST metadata, never the
 * client clock (the encoder box's clock can drift several seconds;
 * an over-shot target anchors at the edge with zero buffer cushion).
 *
 * Decoder-hiccup recovery: broadcast TNT H.264 is field-coded with an
 * MMCO-managed DPB that strains strict platform decoders — Firefox's
 * VideoToolbox decodes it fine for long stretches, then trips on a
 * rough GOP (`AppleVTDecoder OnDecodeError` → media error 3) or
 * silently wedges (playhead frozen inside a buffered range). Both are
 * absorbed IN PLACE: tear down the MediaSource, re-anchor at the live
 * edge, restart the pipeline on the same `<video>` — a fresh decoder
 * session, sub-second glitch, bounded budget. Only when the budget is
 * exhausted does the error surface (the page then rotates sources).
 */

import {
	ALL_FORMATS,
	AudioSampleSink,
	type AudioSampleSource,
	EncodedAudioPacketSource,
	EncodedPacketSink,
	EncodedVideoPacketSource,
	Input,
	Mp4OutputFormat,
	Output,
	StreamTarget,
	type StreamTargetChunk
} from 'mediabunny';

import { isFirefox } from '../caps';
import { planAudioTrack, transcodeSampleSource, type AudioPlan } from '../decode/audio-plan';
import { bindVideoCallbacks, videoBackedHandle, type EngineHandle, type EngineMount } from '../engine';
import { AppendQueue } from '../mse/append-queue';
import { FeedGate } from '../mse/feed-gate';
import { endStream, openMediaSource, releaseVideo } from '../mse/media-source';
import { relaxMediabunnyGopCheck } from '../mse/output';
import { bufferedAhead, evictionSpan, forwardGapTarget } from '../mse/ranges';
import { irisUrlSource } from '../stream-fetch';
import { nalLengthSize, packetHasIdr } from '../decode/h264-idr';

/** How far behind the playlist's end we aim the first keyframe. */
const LIVE_EDGE_BACKOFF_S = 12;
/** Forward feed bound past the playhead (memory cap; the live window is
 *  shallow anyway). */
const AHEAD_TARGET_S = 30;
/** Played-out media kept for pause/rewind-a-bit. */
const BEHIND_KEEP_S = 30;
/** Max seconds one track's feed may lead the other (muxer interleave cap). */
const TRACK_LEAD_CAP = 4;
/** Cap on undrained append chunks held in RAM. */
const MAX_QUEUED_CHUNKS = 16;
/** Played-out media trimmed in steps of at least this much (not a remove() per timeupdate). */
const EVICT_STEP_S = 5;
/** In-place pipeline restarts allowed within the rolling window before the
 *  error surfaces to the page (which then rotates sources). */
const RESTART_BUDGET = 3;
const RESTART_WINDOW_MS = 90_000;
/** Consecutive landed appends with a frozen playhead (while un-paused with
 *  buffer ahead) before we call the decoder wedged and restart. ~16 appends
 *  ≈ several seconds of media landing with zero playback progress. */
const WEDGE_APPEND_LIMIT = 16;

/** Key packets to walk while hunting a true IDR anchor. At broadcast IDR
 *  cadence (~1-4 s) this covers the whole live window and then some. */
const IDR_HUNT_LIMIT = 24;

export const mountTierBLive: EngineMount = async (opts) => {
	const { container, streamUrl } = opts;
	const fail = (err: Error) => opts.onError(err);

	if (typeof globalThis.MediaSource === 'undefined') {
		const err = new Error('MediaSource Extensions not available');
		fail(err);
		throw err;
	}

	container.innerHTML = '';
	const video = document.createElement('video');
	video.className = 'h-full w-full object-contain';
	video.playsInline = true;
	container.appendChild(video);

	const initialSeek = { done: true }; // live: never replay the VOD resume seek
	const unbindVideo = bindVideoCallbacks(video, opts, initialSeek);

	const firefox = isFirefox();
	let disposed = false;
	/** Bumped on every pipeline (re)start; every async loop and sink write
	 *  guards on it so stale cycles die quietly. */
	let generation = 0;
	let mediaSource: MediaSource | null = null;
	let objectUrl: string | null = null;
	let sourceBuffer: SourceBuffer | null = null;
	let output: Output | null = null;
	let input: Input | null = null;
	/** Wall-clock stamps of recent in-place restarts (budget window). */
	const restartStamps: number[] = [];
	/** Anchor of the CURRENT cycle (playlist-relative seconds). */
	let anchor = 0;
	/** Restarts must never re-anchor at (or before) a previous anchor: a
	 *  cycle that died right after starting poisons its own GOP, and the
	 *  stale playlist metadata would otherwise re-pick the exact same
	 *  keyframe forever. Strictly-forward anchoring guarantees progress. */
	let anchorFloor = 0;
	let videoFedMax = 0; // anchor-relative
	let audioFedMax = 0;
	/** Set once the current cycle anchored the playhead + attempted play. */
	let playbackStarted = false;
	/** Wedge detector state (see WEDGE_APPEND_LIMIT). */
	let wedgeLastT = -1;
	let wedgeAppends = 0;

	// waiters (all flushed on dispose AND on restart)

	const gate = new FeedGate();
	const sinkWaiters = new Set<() => void>();
	const flushAllWaiters = () => {
		gate.flush();
		for (const w of sinkWaiters) w();
		sinkWaiters.clear();
	};

	const waitTrackBalance = (gen: number, ts: number, otherFedMax: () => number): Promise<void> =>
		gate.wait(() => disposed || gen !== generation || ts <= otherFedMax() + TRACK_LEAD_CAP);

	const waitBufferRoom = (gen: number, ts: number): Promise<void> =>
		gate.wait(() => disposed || gen !== generation || ts - video.currentTime <= AHEAD_TARGET_S);

	// buffer plumbing

	const bufferedAheadSeconds = (): number => (sourceBuffer ? bufferedAhead(sourceBuffer.buffered, video.currentTime) : 0);

	const evictPlayedRange = (minSpan: number): void => {
		// Firefox: never run our own remove() — it can wedge `updating=true`
		// forever (VOD Tier B lore); FF's native eviction handles the shallow
		// live window fine.
		if (firefox || !sourceBuffer || sourceBuffer.updating) return;
		const span = evictionSpan(sourceBuffer.buffered, video.currentTime, BEHIND_KEEP_S, minSpan);
		if (!span) return;
		try {
			sourceBuffer.remove(span[0], span[1]);
		} catch {
			/* retried on the next tick */
		}
	};

	const appendQueue = new AppendQueue({
		alive: () => !disposed,
		onQuota: () => evictPlayedRange(0),
		onError: (e) => fail(e)
	});

	/** Jump the playhead across a small forward gap. True when it moved. */
	const jumpForwardGap = (): boolean => {
		if (!sourceBuffer) return false;
		const t = video.currentTime;
		const start = forwardGapTarget(sourceBuffer.buffered, t);
		if (start === null) return false;
		console.warn(`[iris-core] live: jumping gap ${t.toFixed(2)} → ${start.toFixed(2)}`);
		try {
			video.currentTime = start + 0.01;
		} catch {
			/* swallow */
		}
		return true;
	};

	// in-place restart

	/** Tear down the current MediaSource + pipeline and start a fresh cycle
	 *  at the live edge, on the SAME `<video>`. Absorbs VideoToolbox decode
	 *  errors / wedges without surfacing to the page. Budget-bounded. */
	const restartPipeline = async (reason: string): Promise<void> => {
		if (disposed) return;
		const now = Date.now();
		while (restartStamps.length > 0 && now - restartStamps[0]! > RESTART_WINDOW_MS) {
			restartStamps.shift();
		}
		if (restartStamps.length >= RESTART_BUDGET) {
			fail(new Error(`live: pipeline restarted ${RESTART_BUDGET}x in ${RESTART_WINDOW_MS / 1000}s — giving up (${reason})`));
			return;
		}
		restartStamps.push(now);
		const wasPlaying = !video.paused;
		console.warn(
			`[iris-core] live: in-place pipeline restart #${restartStamps.length} (${reason}) ` +
				`t=${video.currentTime.toFixed(1)} playing=${wasPlaying}`
		);
		generation += 1;
		flushAllWaiters();
		appendQueue.clear();
		const oldOutput = output;
		output = null;
		try {
			await oldOutput?.cancel();
		} catch {
			/* idempotent */
		}
		if (disposed) return;
		try {
			await startCycle(wasPlaying);
		} catch (e) {
			if (!disposed) fail(e instanceof Error ? e : new Error(String(e)));
		}
	};

	// video element error handling

	// Per-cycle one-shot (Firefox can fire `error` repeatedly). Reset by
	// `startCycle` when it re-arms the element with a fresh MediaSource.
	let elementErrorHandled = false;
	const onErr = () => {
		if (disposed || elementErrorHandled) return;
		elementErrorHandled = true;
		const err = video.error;
		console.warn(`[iris-core] live: media element error code=${err?.code} t=${video.currentTime.toFixed(1)} msg=${err?.message ?? ''}`);
		// MEDIA_ERR_DECODE (3): a platform-decoder trip on a rough broadcast
		// GOP — recover in place with a fresh decoder session. Anything else
		// (src not supported, network on a blob…) is structural: surface it.
		if (err?.code === 3) {
			void restartPipeline('media decode error');
		} else {
			fail(new Error(err ? `media error ${err.code}: ${err.message}` : 'video element error'));
		}
	};
	video.addEventListener('error', onErr);

	// Playhead advanced → trim behind, retry queued appends, release feeds.
	const onTimeUpdate = () => {
		if (disposed) return;
		evictPlayedRange(EVICT_STEP_S);
		if (appendQueue.length > 0) appendQueue.pump();
		gate.notify();
	};
	video.addEventListener('timeupdate', onTimeUpdate);

	// Underrun: unwedge a hung append (FF bug 1120084 lore), jump a gap.
	const onWaiting = () => {
		if (disposed || !sourceBuffer) return;
		if (sourceBuffer.updating) {
			try {
				sourceBuffer.abort();
				console.warn('[iris-core] live: aborted wedged SourceBuffer op (FF unwedge)');
			} catch {
				/* MediaSource not open — dispose path owns it */
			}
		}
		appendQueue.pump();
		jumpForwardGap();
	};
	video.addEventListener('waiting', onWaiting);
	video.addEventListener('stalled', onWaiting);

	// dispose

	const dispose = async (): Promise<void> => {
		if (disposed) return;
		disposed = true;
		generation += 1;
		flushAllWaiters();
		unbindVideo();
		video.removeEventListener('error', onErr);
		video.removeEventListener('waiting', onWaiting);
		video.removeEventListener('stalled', onWaiting);
		video.removeEventListener('timeupdate', onTimeUpdate);
		try {
			await output?.cancel();
		} catch {
			/* idempotent */
		}
		try {
			input?.dispose();
		} catch {
			/* idempotent */
		}
		endStream(mediaSource);
		appendQueue.detach();
		releaseVideo(video, objectUrl);
	};

	// stream probing (once per mount)

	let mime = '';
	let videoDecoderConfigCodec = '';
	let audioPlan: AudioPlan | null = null;

	/** (Re)create the MediaSource + SourceBuffer on the `<video>`, wire the
	 *  per-cycle listeners, then anchor at the live edge and spawn the feed
	 *  loops. Used by the initial mount and by every in-place restart. */
	const startCycle = async (resumePlaying: boolean): Promise<void> => {
		const gen = generation;
		const liveInput = input;
		if (!liveInput) throw new Error('live: startCycle before input init');
		// Phase timing — tells us exactly where a slow start went.
		const t0 = performance.now();
		const phase = (label: string) =>
			console.log(`[iris-core] live cycle #${gen} phase: ${label} +${(performance.now() - t0).toFixed(0)}ms`);

		// Fresh MediaSource on the same element — also clears a fatal element
		// error state (`video.error`) from a previous cycle.
		if (objectUrl) URL.revokeObjectURL(objectUrl);
		const ms = new MediaSource();
		mediaSource = ms;
		sourceBuffer = null;
		objectUrl = URL.createObjectURL(ms);
		video.src = objectUrl;
		elementErrorHandled = false;
		playbackStarted = false;
		wedgeLastT = -1;
		wedgeAppends = 0;

		await openMediaSource(ms, 'live');
		if (disposed || gen !== generation) return;

		const sb = ms.addSourceBuffer(mime);
		sb.mode = 'segments';
		sourceBuffer = sb;
		appendQueue.attach(sb);
		sb.addEventListener('updateend', () => {
			if (disposed || gen !== generation) return;
			appendQueue.pump();
			if (sb.buffered.length === 0) return;
			if (!playbackStarted) {
				// First media landed → anchor the playhead + (re)start playback.
				playbackStarted = true;
				const start = sb.buffered.start(0);
				console.log(`[iris-core] live: first media buffered [${start.toFixed(2)}-${sb.buffered.end(0).toFixed(2)}] — anchoring playhead`);
				opts.onReady?.();
				opts.onReady = undefined;
				try {
					video.currentTime = start + 0.05;
				} catch {
					/* swallow */
				}
				if (resumePlaying || restartStamps.length === 0) {
					void video.play().catch(() => {
						// Autoplay-with-sound blocked (Firefox default policy). Stay
						// paused with the chrome's play button.
						console.warn('[iris-core] live: autoplay blocked — press play');
					});
				}
				return;
			}
			// Stall self-healing. Firefox fires `waiting` once per stall; when
			// recovery found nothing back then, landed appends are the only
			// events left. Jump a gap, or nudge; if appends keep landing while
			// the playhead stays frozen, the decoder is wedged → fresh session.
			if (!video.paused && video.readyState < 3) {
				if (video.currentTime === wedgeLastT) {
					wedgeAppends += 1;
					if (wedgeAppends >= WEDGE_APPEND_LIMIT) {
						wedgeAppends = 0;
						void restartPipeline('decoder wedged (frozen playhead)');
						return;
					}
				} else {
					wedgeLastT = video.currentTime;
					wedgeAppends = 0;
				}
				if (!jumpForwardGap() && bufferedAheadSeconds() > 1) {
					try {
						video.currentTime = video.currentTime + 0.001;
					} catch {
						/* swallow */
					}
				}
			} else {
				wedgeLastT = -1;
				wedgeAppends = 0;
			}
		});
		sb.addEventListener('error', () => {
			if (disposed || gen !== generation) return;
			// The element-level error handler decides recover-vs-surface.
			console.warn('[iris-core] live: SourceBuffer error event');
		});

		// anchor at the live edge (playlist metadata, never client clock)
		phase('MediaSource open');
		const videoTrack = await liveInput.getPrimaryVideoTrack();
		if (!videoTrack) throw new Error('live: no video track');
		const videoPacketSink = new EncodedPacketSink(videoTrack);
		const videoDecoderConfig = await videoTrack.getDecoderConfig();
		const sourceVideoCodec = await videoTrack.getCodec();
		if (!videoDecoderConfig || !sourceVideoCodec) throw new Error('live: video codec unknown');
		phase('video track probed');
		// skipLiveWait is load-bearing: without it this resolves only once the
		// live stream ENDS (it follows the growing playlist forever).
		const windowEnd = (await liveInput.getDurationFromMetadata(undefined, { skipLiveWait: true })) ?? 0;
		const edgeTarget = Math.max(0, windowEnd - LIVE_EDGE_BACKOFF_S, anchorFloor);
		let startPacket = await videoPacketSink.getKeyPacket(edgeTarget);
		if (!startPacket) startPacket = await videoPacketSink.getFirstKeyPacket();
		if (!startPacket) throw new Error('live: no video keyframe in window');
		phase(`edge keyframe found (target=${edgeTarget.toFixed(1)}s)`);
		// Walk forward to a true IDR: fMP4 sync samples include open-GOP
		// recovery points that strict decoders can't start on (see
		// `packetHasIdr`). Each step at the live edge can BLOCK for a whole
		// segment duration (the sink follows the growing playlist) — the
		// per-step log tells us when startup time is going into this hunt.
		const lenSize = nalLengthSize((await videoTrack.getDecoderConfig())?.description as BufferSource | undefined);
		let hunted = 0;
		let idrPacket = startPacket;
		while (hunted < IDR_HUNT_LIMIT && !packetHasIdr(idrPacket.data, lenSize)) {
			if (disposed || gen !== generation) return;
			const stepStart = performance.now();
			const next = await videoPacketSink.getNextKeyPacket(idrPacket);
			const stepMs = performance.now() - stepStart;
			if (stepMs > 500) {
				console.log(`[iris-core] live: IDR hunt step ${hunted + 1} blocked ${stepMs.toFixed(0)}ms (waiting on live segments)`);
			}
			if (!next) break;
			idrPacket = next;
			hunted += 1;
		}
		phase(`IDR hunt done (${hunted} steps)`);
		if (packetHasIdr(idrPacket.data, lenSize)) {
			startPacket = idrPacket;
		} else {
			console.warn(
				`[iris-core] live: no IDR found within ${IDR_HUNT_LIMIT} keyframes — ` +
					`anchoring on a recovery point (strict decoders may object)`
			);
		}
		if (disposed || gen !== generation) return;
		anchor = startPacket.timestamp;
		anchorFloor = anchor + 0.5;
		videoFedMax = 0;
		audioFedMax = 0;
		console.log(
			`[iris-core] live cycle #${gen}: window=${windowEnd.toFixed(1)}s ` +
				`anchor=${anchor.toFixed(1)}s cushion=${(windowEnd - anchor).toFixed(1)}s ` +
				`idrHunt=${hunted}`
		);

		// output + sink
		let sinkChunks = 0;
		const newOutput = new Output({
			format: new Mp4OutputFormat({ fastStart: 'fragmented', minimumFragmentDuration: 1 }),
			target: new StreamTarget(
				new WritableStream<StreamTargetChunk>({
					write: async (chunk) => {
						if (disposed || gen !== generation) return;
						sinkChunks += 1;
						if (sinkChunks <= 2) {
							console.log(`[iris-core] live: muxer chunk #${sinkChunks} (${chunk.data.byteLength} bytes)`);
						}
						appendQueue.push(chunk.data);
						appendQueue.pump();
						// Park until an append lands or playback drains the buffer —
						// event-driven; dispose/restart flushes `sinkWaiters`.
						while (!disposed && gen === generation && (bufferedAheadSeconds() > AHEAD_TARGET_S || appendQueue.length > MAX_QUEUED_CHUNKS)) {
							await new Promise<void>((resolve) => {
								if (!sourceBuffer) {
									resolve();
									return;
								}
								let settled = false;
								const done = () => {
									if (settled) return;
									settled = true;
									sourceBuffer?.removeEventListener('updateend', done);
									video.removeEventListener('timeupdate', done);
									sinkWaiters.delete(done);
									resolve();
								};
								sinkWaiters.add(done);
								sourceBuffer.addEventListener('updateend', done, { once: true });
								video.addEventListener('timeupdate', done, { once: true });
							});
						}
					},
					close: () => {
						// ENDLIST (the backend session died) → let the element end; the
						// page's onEnded handler rotates to the next source.
						if (disposed || gen !== generation) return;
						endStream(ms);
					},
					abort: (reason) => {
						if (disposed || gen !== generation) return;
						fail(reason instanceof Error ? reason : new Error(String(reason)));
					}
				})
			)
		});
		relaxMediabunnyGopCheck(newOutput);
		output = newOutput;

		const videoSrc = new EncodedVideoPacketSource(sourceVideoCodec);
		newOutput.addVideoTrack(videoSrc);

		const audioTrack = (await liveInput.getAudioTracks())[0] ?? null;
		type AudioFeed = { kind: 'passthrough'; source: EncodedAudioPacketSource } | { kind: 'transcode'; source: AudioSampleSource };
		let audioFeed: AudioFeed | null = null;
		if (audioTrack && audioPlan) {
			if (audioPlan.kind === 'transcode') {
				const source = transcodeSampleSource(
					{ codec: audioPlan.targetCodec, channels: audioPlan.channels },
					await audioTrack.getNumberOfChannels()
				);
				newOutput.addAudioTrack(source);
				audioFeed = { kind: 'transcode', source };
			} else {
				const sourceAudioCodec = await audioTrack.getCodec();
				if (sourceAudioCodec) {
					const source = new EncodedAudioPacketSource(sourceAudioCodec);
					newOutput.addAudioTrack(source);
					audioFeed = { kind: 'passthrough', source };
				}
			}
		}

		await newOutput.start();
		if (disposed || gen !== generation) return;

		// feed loops (anchor-relative timestamps into the muxer)

		const startTs = startPacket.timestamp;
		const videoP = (async () => {
			let firstMeta = true;
			let lastLogged = 0;
			for await (const packet of videoPacketSink.packets(startPacket)) {
				if (disposed || gen !== generation) break;
				// Skip the first keyframe's own leading pictures (undecodable at
				// random access — same rule as the VOD engine).
				if (packet.timestamp < startTs) continue;
				const rel = packet.timestamp - anchor;
				await waitTrackBalance(gen, rel, () => audioFedMax);
				if (disposed || gen !== generation) break;
				await waitBufferRoom(gen, rel);
				if (disposed || gen !== generation) break;
				await videoSrc.add(packet.clone({ timestamp: rel }), firstMeta ? { decoderConfig: videoDecoderConfig } : undefined);
				firstMeta = false;
				if (disposed || gen !== generation) break;
				if (rel > videoFedMax) videoFedMax = rel;
				if (rel - lastLogged >= 5) {
					lastLogged = rel;
					const q = video.getVideoPlaybackQuality?.();
					console.log(
						`[iris-core] live: fed v=${videoFedMax.toFixed(1)}s a=${audioFedMax.toFixed(1)}s ` +
							`t=${video.currentTime.toFixed(1)}s ahead=${bufferedAheadSeconds().toFixed(1)}s ` +
							`queue=${appendQueue.length} chunks=${sinkChunks} rs=${video.readyState}` +
							(q ? ` frames=${q.totalVideoFrames}/drop=${q.droppedVideoFrames}` : '')
					);
				}
				gate.notify();
			}
			try {
				await videoSrc.close();
			} catch {
				/* output cancelled mid-flush — teardown noise */
			}
			if (disposed || gen !== generation) return;
			videoFedMax = Number.POSITIVE_INFINITY;
			gate.notify();
		})();

		if (!(audioTrack && audioFeed)) {
			audioFedMax = Number.POSITIVE_INFINITY;
			gate.notify();
		}
		const audioP =
			audioTrack && audioFeed
				? (async () => {
						const feed = audioFeed;
						if (feed.kind === 'passthrough') {
							const packetSink = new EncodedPacketSink(audioTrack);
							let start = await packetSink.getKeyPacket(anchor);
							if (!start) start = await packetSink.getFirstKeyPacket();
							if (!start) {
								try {
									await feed.source.close();
								} catch {
									/* output cancelled mid-flush — teardown noise */
								}
								if (disposed || gen !== generation) return;
								audioFedMax = Number.POSITIVE_INFINITY;
								gate.notify();
								return;
							}
							const decoderConfig = await audioTrack.getDecoderConfig();
							let firstMeta = true;
							for await (const packet of packetSink.packets(start)) {
								if (disposed || gen !== generation) break;
								// Clamp: the first packet can start a frame before the anchor.
								const rel = Math.max(0, packet.timestamp - anchor);
								await waitTrackBalance(gen, rel, () => videoFedMax);
								if (disposed || gen !== generation) break;
								await waitBufferRoom(gen, rel);
								if (disposed || gen !== generation) break;
								await feed.source.add(
									packet.clone({ timestamp: rel }),
									firstMeta ? { decoderConfig: decoderConfig ?? undefined } : undefined
								);
								firstMeta = false;
								if (disposed || gen !== generation) break;
								if (rel > audioFedMax) audioFedMax = rel;
								gate.notify();
							}
							try {
								await feed.source.close();
							} catch {
								/* output cancelled mid-flush — teardown noise */
							}
						} else {
							const sampleSink = new AudioSampleSink(audioTrack);
							for await (const sample of sampleSink.samples(anchor, Number.POSITIVE_INFINITY)) {
								if (disposed || gen !== generation) {
									sample.close();
									break;
								}
								// Clamp: the first sample can start a frame before the anchor.
								const rel = Math.max(0, sample.timestamp - anchor);
								try {
									await waitTrackBalance(gen, rel, () => videoFedMax);
									if (disposed || gen !== generation) break;
									await waitBufferRoom(gen, rel);
									if (disposed || gen !== generation) break;
									sample.setTimestamp(rel);
									await feed.source.add(sample);
									if (disposed || gen !== generation) break;
									if (rel > audioFedMax) audioFedMax = rel;
									gate.notify();
								} finally {
									sample.close();
								}
							}
							try {
								await feed.source.close();
							} catch {
								/* output cancelled mid-flush — teardown noise */
							}
						}
						if (disposed || gen !== generation) return;
						audioFedMax = Number.POSITIVE_INFINITY;
						gate.notify();
					})()
				: Promise.resolve();

		void Promise.all([videoP, audioP])
			.then(() => {
				if (disposed || gen !== generation) return;
				return newOutput.finalize();
			})
			.catch((e: unknown) => {
				if (disposed || gen !== generation) return;
				if (e instanceof Error && /cancel/i.test(e.message)) return;
				fail(e instanceof Error ? e : new Error(String(e)));
			});
	};

	// mount: probe once, then run the first cycle

	const mount0 = performance.now();
	try {
		input = new Input({
			// a source rotation briefly 502s while the backend elects the next feed
			source: irisUrlSource(streamUrl, { cacheBytes: 32 * 1024 * 1024, attempts: 6, maxDelayS: 4 }),
			formats: ALL_FORMATS,
			// Gapless continuous timeline — see the module header. Wall-clock
			// times stay reachable via `InputTrack.getUnixTimeForTimestamp`.
			formatOptions: { hls: { offsetTimestampsByDateTime: false } }
		});

		const videoTrack = await input.getPrimaryVideoTrack();
		if (!videoTrack) throw new Error('live: no video track in stream');
		const videoDecoderConfig = await videoTrack.getDecoderConfig();
		if (!videoDecoderConfig?.codec) throw new Error('live: video codec unknown');
		videoDecoderConfigCodec = videoDecoderConfig.codec;

		const audioTrack = (await input.getAudioTracks())[0] ?? null;
		const audioCodec = audioTrack ? await audioTrack.getCodec() : null;
		audioPlan = audioTrack ? await planAudioTrack(audioTrack, 'live') : null;

		const codecs = [videoDecoderConfigCodec, audioPlan?.mp4Codec].filter(Boolean).join(',');
		mime = `video/mp4; codecs="${codecs}"`;
		if (!MediaSource.isTypeSupported(mime)) {
			throw new Error(`live: MIME not supported by MSE: ${mime}`);
		}
		console.log(
			`[iris-core] live mount: video=${videoDecoderConfigCodec} audio=${audioCodec ?? 'none'}` +
				`${audioPlan ? ` (${audioPlan.kind} → ${audioPlan.mp4Codec})` : ''} ` +
				`probeMs=${(performance.now() - mount0).toFixed(0)}`
		);

		await startCycle(true);
	} catch (e) {
		await dispose();
		const err = e instanceof Error ? e : new Error(String(e));
		fail(err);
		throw err;
	}

	const handle: EngineHandle = videoBackedHandle(video, {
		dispose,
		fallbackDuration: null,
		audioTracks: () => []
	});
	return handle;
};
