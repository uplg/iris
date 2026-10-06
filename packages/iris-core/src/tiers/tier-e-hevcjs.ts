/**
 * Tier E — hevc.js, split-track pipeline.
 *
 * Mediabunny demuxes the source and remuxes it into TWO fragmented MP4
 * streams, each single-track, feeding two SourceBuffers on one MediaSource:
 *
 *   video (HEVC) → SourceBuffer("video/mp4; codecs=hvc1…")  ← hevc.js proxy
 *                  the proxy transcodes to H.264 in a WASM worker, so the
 *                  browser only ever sees `avc1…`
 *   audio        → SourceBuffer("audio/mp4; codecs=…")      ← native, untouched
 *
 * Why split rather than reuse Tier B's single muxed SourceBuffer: hevc.js's
 * muxed path is AAC-only ("the AAC audio is passed through … main-thread path;
 * AAC only"), and Firefox has no AAC in WebCodecs — Tier B falls back to Opus
 * there. Handing the muxed proxy an Opus stream makes it create a real buffer
 * for `avc1…,mp4a.40.2` and then queue every append forever, without an error.
 * Video alone through the proxy sidesteps that entirely.
 *
 * Why this tier exists on macOS at all, where hevc.js's own matrix says
 * "No — native": the problem there is not decoding HEVC, it is *entering* an
 * HEVC stream. Gecko 154+ strips the keyframe flag from CRA pictures
 * (`MP4Demuxer.cpp`, bug 2049615), and an open-GOP rip has one IDR, at t=0 — so
 * MSE silently drops every mid-stream start and `buffered` stays empty. Routing
 * through hevc.js means Gecko sees H.264 and the guard never applies.
 * See `web/tools/mse-bisect/README.md` for the measurements.
 */

import {
	ALL_FORMATS,
	AudioSampleSink,
	type AudioSampleSource,
	EncodedPacket,
	EncodedPacketSink,
	EncodedVideoPacketSource,
	Input,
	Mp4OutputFormat,
	Output,
	type StreamTargetChunk,
	StreamTarget,
	EncodedAudioPacketSource
} from 'mediabunny';

import { isFirefox } from '../caps';
import {
	appendNativeTrack,
	bindVideoCallbacks,
	defaultAudioIndex,
	videoBackedHandle,
	type EngineHandle,
	type EngineMount
} from '../engine';
import { ensureLibavAudioDecoderRegistered } from '../decode/libav-audio-decoder';
import { libavCanDecode } from '../decode/libav-codecs';
import { pickAudioEncoder, transcodeSampleSource } from '../decode/audio-plan';
import { AppendQueue } from '../mse/append-queue';
import { FeedGate } from '../mse/feed-gate';
import { initSegmentEnd } from '../mse/fmp4';
import { endStream, openMediaSource, releaseVideo } from '../mse/media-source';
import { relaxMediabunnyGopCheck } from '../mse/output';
import { aheadInRange, coversTime, evictionSpan, landsAt } from '../mse/ranges';
import { irisUrlSource, VOD_RETRY } from '../stream-source';

/** `subscribeSegmentStat` from `@hevcjs/core`, captured on first load. The lib
 *  publishes one stat per transcoded segment, `speedX` being media-seconds
 *  produced per wall-second — the only honest measure of whether the WASM
 *  decoder is keeping up on this machine right now. */
let subscribeSegmentStat: ((l: (s: { speedX: number }) => void) => () => void) | null = null;

const WASM_URL = '/hevcjs/hevc-decode.js';
const WASM_BINARY_URL = '/hevcjs/hevc-decode.wasm';
const WORKER_URL = '/hevcjs/transcode-worker.js';

/** Seconds of media the feeds may run ahead of the playhead — the runway the
 *  transcoder has to absorb a spell of CPU contention without the playhead
 *  catching up with it. Sized from measured throughput between these bounds:
 *  a transcoder with plenty of headroom needs almost none, one running close
 *  to real time needs all it can get. Too deep only wastes work on a seek. */
const AHEAD_MIN_S = 30;
const AHEAD_MAX_S = 90;
/** How far one track may run ahead of the other before it waits. */
const TRACK_LEAD_CAP_S = 4;
/** Fragments the muxer emits. Short ones matter more here than in Tier B: the
 *  proxy cannot start transcoding until a whole fragment has arrived, so this
 *  is the floor on startup and post-seek latency. */
const FRAGMENT_S = 0.5;
/** Cadence at which we force a fragment boundary — see the video pump. Ramped:
 *  the proxy transcodes a whole fragment before emitting anything, so the first
 *  one after a seek sets the time-to-first-frame and wants to be short, while
 *  steady-state fragments want to be long enough to amortise the per-segment
 *  demux/mux/postMessage cost. */
const FORCED_BOUNDARY_START_S = 0.75;
const FORCED_BOUNDARY_MAX_S = 3;
/** Snap the playhead back to the keyframe when the requested position is more
 *  than this far past it. */
const KEYFRAME_SNAP_S = 2;
/** Ceiling on media handed to the proxy but not yet transcoded. The proxy
 *  takes appends eagerly, so without this the feed races ahead of the worker
 *  and parks tens of seconds of compressed HEVC in its queue: memory pressure
 *  that slows the very transcode it is waiting on. The cushion has to be built
 *  out of transcoded output, not backlog.
 *
 *  This is also what makes a restart cheap. The stale backlog a seek leaves
 *  behind is bounded by this, so there is no need to reach into the proxy and
 *  flush it. */
const IN_FLIGHT_CAP_S = 8;
/** Cushion to build before letting playback start after a mount or a seek. The
 *  transcoder is slowest exactly when it is coldest — worker spin-up, decoder
 *  init, an empty pipeline — so starting the moment two frames exist means
 *  playback immediately outruns it and stutters until the two rates cross. One
 *  honest wait under the spinner is better than thirty seconds of hiccups. */
const STARTUP_CUSHION_S = 6;
/** Same idea after a mid-playback rebuffer, where the pipeline is already warm
 *  and only needs to get back in front of the playhead. */
const REBUFFER_CUSHION_S = 4;
/** Rolling window over per-segment throughput, in segments. At the forced
 *  boundary cadence that is roughly the last 12 s of media. */
const SPEED_WINDOW = 8;
/** Played-out audio kept in its SourceBuffer, trimmed in steps (not a remove() per tick). */
const PLAYED_KEEP_S = 30;
const EVICT_STEP_S = 5;
/** Mediabunny's read cache, capped well under its 64 MiB default. */
const SOURCE_CACHE_BYTES = 16 * 1024 * 1024;

let intercept: { install: () => void; uninstall: () => void } | null = null;
let installed = false;
let installCount = 0;

/** Publish the WASM decoder factory as `globalThis.HEVCDecoderModule`.
 *
 *  hevc.js resolves its decoder as `globalThis.HEVCDecoderModule` first, then
 *  falls back to `await import(wasmUrl)` and `mod.default ?? mod`. The file the
 *  package publishes — and that `sync-vendor` copies into `public/hevcjs/` — is
 *  the IIFE/UMD build: a browser `import()` of it yields a namespace with no
 *  `default` and the call throws. A classic `<script>` is what that build is
 *  for; it assigns the global and the import is never reached. */
function ensureDecoderGlobal(): Promise<void> {
	const g = globalThis as { HEVCDecoderModule?: unknown };
	if (typeof g.HEVCDecoderModule === 'function') return Promise.resolve();
	const existing = document.querySelector<HTMLScriptElement>(`script[src="${WASM_URL}"]`);
	const el = existing ?? document.createElement('script');
	const done = new Promise<void>((resolve, reject) => {
		el.addEventListener('load', () => resolve(), { once: true });
		el.addEventListener('error', () => reject(new Error(`Tier E: failed to load ${WASM_URL}`)), {
			once: true
		});
	});
	if (!existing) {
		el.src = WASM_URL;
		el.async = true;
		document.head.appendChild(el);
	}
	return done;
}

async function ensureIntercept(): Promise<void> {
	if (!intercept) {
		// Lazy-load the lib so only Tier E sessions pay the ~70 KB cost.
		const mod = await import('@hevcjs/core');
		subscribeSegmentStat = mod.subscribeSegmentStat;
		await ensureDecoderGlobal();
		intercept = {
			install: () =>
				mod.installMSEIntercept({
					wasmUrl: WASM_URL,
					wasmBinaryUrl: WASM_BINARY_URL,
					workerUrl: WORKER_URL,
					logLevel: 'warn'
				}),
			uninstall: () => mod.uninstallMSEIntercept()
		};
	}
	if (!installed) {
		intercept.install();
		installed = true;
	}
	installCount += 1;
}

function releaseIntercept(): void {
	if (!intercept || !installed) return;
	installCount = Math.max(0, installCount - 1);
	if (installCount === 0) {
		intercept.uninstall();
		installed = false;
	}
}

/** Re-frame mediabunny's chunk stream for hevc.js.
 *
 *  Mediabunny writes `ftyp` as its own 28-byte chunk, then a second chunk
 *  carrying `moov` followed by the first `moof`+`mdat`. hevc.js decides what a
 *  chunk is with `isInitSegment`, which only looks at the first box type — so a
 *  lone `ftyp` is accepted as a complete init segment, handed to the
 *  transcoder, and the queue then stalls with no error and no "Init segment
 *  parsed". Every later append piles up behind it.
 *
 *  So hand it what it expects: one append containing `ftyp`+`moov`, then media
 *  segments. This accumulates until the `moov` is complete, emits the pair, and
 *  passes everything after through untouched. */
class InitFramer {
	private pending: Uint8Array[] = [];
	private pendingBytes = 0;
	private initDone = false;

	/** Returns the buffers to append, in order. */
	push(chunk: Uint8Array): Uint8Array[] {
		if (this.initDone) return [chunk];
		this.pending.push(chunk);
		this.pendingBytes += chunk.byteLength;
		const joined = new Uint8Array(this.pendingBytes);
		let at = 0;
		for (const part of this.pending) {
			joined.set(part, at);
			at += part.byteLength;
		}
		const end = initSegmentEnd(joined);
		if (end < 0) return []; // `moov` not complete yet — keep accumulating
		this.initDone = true;
		this.pending = [];
		this.pendingBytes = 0;
		const rest = joined.subarray(end);
		return rest.byteLength > 0 ? [joined.subarray(0, end), rest] : [joined.subarray(0, end)];
	}
}

/** One SourceBuffer plus the queue feeding it. */
type Lane = {
	name: 'video' | 'audio';
	sb: SourceBuffer;
	queue: AppendQueue;
	fedMax: number;
	ended: boolean;
	/** Diagnostics: how many appends landed, and whether we logged first data. */
	appended: number;
	reported: boolean;
};

export const mountTierE: EngineMount = async (opts) => {
	const { container, manifest, streamUrl, nativeSubs, audioTrackIndex } = opts;

	if (typeof globalThis.MediaSource === 'undefined') {
		throw new Error('Tier E: MediaSource is not available');
	}
	const videoCodecString = manifest.video[0]?.codec_string;
	if (!videoCodecString) throw new Error('Tier E: manifest has no video codec string');

	// Before the first await: a mount resuming after one must never wipe the element of an
	// engine mounted meanwhile. Each engine removes its own element when disposed.
	container.innerHTML = '';

	const chosenAudioIdx = audioTrackIndex ?? defaultAudioIndex(manifest);
	const chosenAudio = manifest.audio[chosenAudioIdx] ?? null;
	const audioNeedsTranscode = chosenAudio !== null && chosenAudio !== undefined && !chosenAudio.browser_native;
	if (audioNeedsTranscode && !libavCanDecode(chosenAudio.codec)) {
		throw new Error(`Tier E: audio codec ${chosenAudio.codec} not transcodable client-side`);
	}
	if (audioNeedsTranscode) ensureLibavAudioDecoderRegistered();

	let encoderChoice: Awaited<ReturnType<typeof pickAudioEncoder>> = null;
	if (audioNeedsTranscode && chosenAudio) {
		encoderChoice = await pickAudioEncoder(chosenAudio.channels, chosenAudio.sample_rate ?? 48000);
		if (!encoderChoice) {
			throw new Error('Tier E: no usable AudioEncoder for this source');
		}
	}
	const audioMp4Codec = audioNeedsTranscode ? (encoderChoice?.mp4Codec ?? 'mp4a.40.2') : chosenAudio?.codec_string;

	const firefox = isFirefox();
	let disposed = false;
	let generation = 0;
	let videoLane: Lane | null = null;
	let audioLane: Lane | null = null;
	let videoOutput: Output | null = null;
	let audioOutput: Output | null = null;
	let input: Input | null = null;
	/** Playhead to apply once the video lane has buffered it. Setting
	 *  `currentTime` before any data exists leaves Firefox in a pending seek. */
	let pendingAnchor: number | null = null;
	/** Everything below is acquired step by step; `dispose` releases whatever exists, so a
	 *  failed mount gives it all back, the page-wide MSE intercept first (the demotion target,
	 *  hls.js on this same page, would otherwise run its SourceBuffers through it). */
	let holdsIntercept = false;
	let mediaSource: MediaSource | null = null;
	let objectUrl: string | null = null;
	let unbindVideo: () => void = () => undefined;
	let unsubscribeSpeed: (() => void) | undefined;

	const fail = (e: Error) => {
		if (disposed) return;
		opts.onError?.(e);
	};

	const video = document.createElement('video');
	video.className = 'h-full w-full object-contain';
	video.playsInline = true;
	const nativeTrackMap = new Map<number, HTMLTrackElement>();
	for (const sub of nativeSubs) appendNativeTrack(video, sub, nativeTrackMap);

	// The element's own `waiting`/`playing` events cover steady-state buffering,
	// but they say nothing before playback has ever started — and that is exactly
	// the window where this tier makes the user wait longest, transcoding the
	// first fragment. Report it explicitly.
	let busy = false;
	const setBusy = (next: boolean) => {
		if (busy === next || disposed) return;
		busy = next;
		opts.onBusyChange?.(next);
	};

	const gate = new FeedGate();
	const effectivePlayhead = (): number => pendingAnchor ?? video.currentTime;

	const applyPendingAnchor = (): void => {
		const t = pendingAnchor;
		if (t === null || !videoLane || !landsAt(videoLane.sb.buffered, t)) return;
		pendingAnchor = null;
		try {
			if (Math.abs(video.currentTime - t) > 0.05) video.currentTime = t;
		} catch {
			/* swallow */
		}
	};

	// Playback hold. `holdUntil` is the cushion in seconds that has to exist
	// before the element is allowed to run; 0 means no hold. `heldPaused` is the
	// viewer's intent to play while the hold keeps the element paused.
	let holdUntil = STARTUP_CUSHION_S;
	let heldPaused = false;
	const cushion = (): number => (videoLane ? aheadInRange(videoLane.sb.buffered, effectivePlayhead()) : 0);
	const enforceHold = () => {
		if (disposed || holdUntil <= 0) return;
		if (cushion() >= holdUntil) {
			holdUntil = 0;
			setBusy(false);
			if (heldPaused) {
				heldPaused = false;
				void video.play().catch(() => {});
			}
			return;
		}
		if (!video.paused) {
			heldPaused = true;
			video.pause();
			setBusy(true);
		}
	};

	/** The audio lane is a plain SourceBuffer (only video goes through the proxy): trim what
	 *  has played, except on Firefox, whose remove() can wedge (Tier B lore) and which evicts on
	 *  its own. */
	const evictPlayedAudio = (minSpan: number) => {
		const sb = audioLane?.sb;
		if (firefox || !sb || sb.updating) return;
		const span = evictionSpan(sb.buffered, video.currentTime, PLAYED_KEEP_S, minSpan);
		if (!span) return;
		try {
			sb.remove(span[0], span[1]);
		} catch {
			/* retried on the next timeupdate */
		}
	};

	const onTranscodeError = () => fail(new Error('Tier E: hevc.js failed to transcode a segment (see the [hevc.js] log)'));

	const makeLane = (ms: MediaSource, name: 'video' | 'audio', mime: string): Lane => {
		const sb = ms.addSourceBuffer(mime);
		sb.mode = 'segments';
		const lane: Lane = {
			name,
			sb,
			// The hevc.js proxy fires `updateend` as soon as it has *queued* the data, not when
			// the transcode lands, and its `updating` stays false throughout — so the queue only
			// serialises our own calls, which is all `appendBuffer` requires. The transcode
			// back-pressure comes from the feed gates instead. A quota error waits for the next
			// event (no retry loop: that one hung the tab).
			queue: new AppendQueue({
				alive: () => !disposed,
				proxied: true,
				onQuota: () => {
					if (name === 'audio') evictPlayedAudio(0);
				},
				onError: (e) => fail(e),
				onSettled: () => {
					lane.appended += 1;
					if (!lane.reported && lane.sb.buffered.length > 0) {
						lane.reported = true;
						const b = lane.sb.buffered;
						console.log(
							`[iris-core] Tier E: ${lane.name} lane first buffered ` +
								`[${b.start(0).toFixed(1)}-${b.end(0).toFixed(1)}] after ${lane.appended} appends`
						);
					}
					applyPendingAnchor();
					if (lane.name === 'video') enforceHold();
				}
			}),
			fedMax: 0,
			ended: false,
			appended: 0,
			reported: false
		};
		lane.queue.attach(sb);
		// The proxy reports a failed transcode only as an `error` event on the buffer (its
		// message goes to the `[hevc.js]` log), then takes the next append as if nothing
		// happened: a dead H.264 encoder (Firefox with no OpenH264) fails every segment
		// ("Encoder must be configured first") while the player holds forever. The tier is done.
		if (name === 'video') sb.addEventListener('error', onTranscodeError);
		return lane;
	};

	const sinkFor = (lane: Lane, gen: number): WritableStream<StreamTargetChunk> => {
		// Only the video lane goes through the hevc.js proxy, and only it needs the
		// init segment delivered whole. The audio lane is a plain SourceBuffer and
		// takes mediabunny's chunking as-is.
		const framer = lane.name === 'video' ? new InitFramer() : null;
		let firstChunk = true;
		return new WritableStream<StreamTargetChunk>({
			write: (chunk) => {
				if (disposed || gen !== generation) return;
				if (firstChunk) {
					firstChunk = false;
					console.log(`[iris-core] Tier E: ${lane.name} muxer emitted its first chunk (${chunk.data.byteLength}B) for gen ${gen}`);
				}
				const parts = framer ? framer.push(chunk.data) : [chunk.data];
				if (parts.length === 0) return;
				lane.queue.push(...parts);
				lane.queue.pump();
			}
		});
	};

	// Throughput-sized runway. `speedX` is media-seconds transcoded per
	// wall-second; `speedX - 1` is the rate at which the cushion grows during
	// playback. The thinner that headroom, the longer a cushion we need to ride
	// out a dip — so the target is inversely proportional to it, and a
	// comfortable transcoder keeps a small window (less work thrown away on a
	// seek, less memory held). Clamped at both ends.
	let aheadTarget = AHEAD_MIN_S;
	const speeds: number[] = [];
	let lastSlowLogT = -Infinity;

	// Wake the feed gates and retry stalled appends. `timeupdate` covers steady playback; the
	// rest cover a stalled element, where the buffered ranges still grow as the worker drains
	// its queue and nothing else would tell us. All event-driven — no polling.
	const onProgress = () => {
		enforceHold();
		evictPlayedAudio(EVICT_STEP_S);
		videoLane?.queue.pump();
		audioLane?.queue.pump();
		gate.notify();
	};
	// A mid-playback starvation means the transcoder fell behind. Resuming on the
	// two frames that unblock `canplay` just starves again a second later; get
	// back in front of the playhead first.
	const onStarved = () => {
		if (holdUntil <= 0 && !video.paused && video.currentTime > 0) {
			holdUntil = REBUFFER_CUSHION_S;
		}
		enforceHold();
		gate.notify();
	};
	const WAKE_EVENTS = ['timeupdate', 'progress', 'waiting', 'stalled', 'canplay', 'playing'];

	const cancelPipelines = async (): Promise<void> => {
		const prevVideo = videoOutput;
		const prevAudio = audioOutput;
		videoOutput = null;
		audioOutput = null;
		for (const out of [prevVideo, prevAudio]) {
			try {
				await out?.cancel();
			} catch {
				/* cancelled is expected */
			}
		}
	};

	const dispose = async (): Promise<void> => {
		if (disposed) return;
		disposed = true;
		opts.onBusyChange?.(false);
		gate.flush();
		unsubscribeSpeed?.();
		for (const e of WAKE_EVENTS) video.removeEventListener(e, onProgress);
		video.removeEventListener('waiting', onStarved);
		unbindVideo();
		await cancelPipelines();
		try {
			input?.dispose();
		} catch {
			/* idempotent */
		}
		if (videoLane) {
			videoLane.sb.removeEventListener('error', onTranscodeError);
			// the proxy's own abort empties its queue and resets its worker: no segment left
			// to fail over and over once the tier is gone
			try {
				videoLane.sb.abort();
			} catch {
				/* a closed MediaSource refuses; the proxy has cleared its queue by then */
			}
		}
		endStream(mediaSource);
		videoLane?.queue.detach();
		audioLane?.queue.detach();
		releaseVideo(video, objectUrl);
		if (holdsIntercept) {
			holdsIntercept = false;
			releaseIntercept();
		}
	};

	const initialSeek = { done: false };

	const startPipeline = async (seekStart: number): Promise<void> => {
		setBusy(true);
		holdUntil = STARTUP_CUSHION_S;
		// Claim the target NOW, before the demuxer has even been asked where the
		// keyframe is. `effectivePlayhead` is what the chrome's scrubber reads, so
		// without this the position falls back to the pre-seek one for the whole
		// transcode and the seek reads as "it came back". Refined to the keyframe
		// below, once we know it.
		pendingAnchor = seekStart > 0 ? seekStart : null;
		generation += 1;
		const gen = generation;
		/** A newer seek (or dispose) took over: after every await the run checks this and
		 *  bows out, cancelling what it built, so it can't move the anchor back to its own
		 *  target or leave its Outputs running unowned. */
		const stale = () => disposed || gen !== generation;
		gate.flush();
		await cancelPipelines();
		if (stale()) return;

		for (const lane of [videoLane, audioLane]) {
			if (!lane) continue;
			lane.queue.clear();
			lane.fedMax = seekStart;
			lane.ended = false;
			lane.reported = false;
		}

		const liveInput = input;
		if (!liveInput) throw new Error('Tier E: input not initialised');

		const videoTrack = await liveInput.getPrimaryVideoTrack();
		if (stale()) return;
		if (!videoTrack) throw new Error('Tier E: no primary video track');
		const videoCodec = await videoTrack.getCodec();
		if (stale()) return;
		if (!videoCodec) throw new Error('Tier E: unknown video codec');

		const packetSink = new EncodedPacketSink(videoTrack);
		const startPacket = (await packetSink.getKeyPacket(seekStart)) ?? (await packetSink.getFirstKeyPacket());
		if (stale()) return;
		if (!startPacket) throw new Error('Tier E: no keyframe found');
		// `getKeyPacket` lands at or before the target, so both feeds start there
		// and the muxer never has to pad a late track.
		const mediaStart = startPacket.timestamp;
		if (videoLane) videoLane.fedMax = mediaStart;
		if (audioLane) audioLane.fedMax = mediaStart;
		// Land on the keyframe rather than the requested position when the gap is
		// wide. The decoder must start at `mediaStart` either way, and the WASM
		// transcode runs at roughly 0.8x realtime — so honouring an exact target
		// means watching a blank player while a whole GOP is transcoded and thrown
		// away. Measured on this file: 13.8 s to first frame anchored at the target
		// versus the first fragment landing in ~2 s. A seek that lands a few
		// seconds early beats a seek that looks broken; Tier B keeps exact
		// positioning because hardware decode makes the lead-in free.
		const gap = seekStart - mediaStart;
		pendingAnchor = seekStart > 0 ? (gap > KEYFRAME_SNAP_S ? mediaStart : seekStart) : null;
		if (gap > KEYFRAME_SNAP_S) {
			console.log(
				`[iris-core] Tier E: snapping ${seekStart.toFixed(1)}s → keyframe ` +
					`${mediaStart.toFixed(1)}s (${gap.toFixed(1)}s of lead-in would transcode first)`
			);
		}

		const vOut = new Output({
			format: new Mp4OutputFormat({ fastStart: 'fragmented', minimumFragmentDuration: FRAGMENT_S }),
			target: new StreamTarget(sinkFor(videoLane!, gen))
		});
		relaxMediabunnyGopCheck(vOut);
		const videoSrc = new EncodedVideoPacketSource(videoCodec);
		vOut.addVideoTrack(videoSrc);
		let aOut: Output | null = null;
		const bail = () => {
			void vOut.cancel().catch(() => undefined);
			void aOut?.cancel().catch(() => undefined);
		};

		const allAudio = await liveInput.getAudioTracks();
		if (stale()) return bail();
		const audioTrack = allAudio[chosenAudioIdx] ?? null;
		let audioSrc: AudioSampleSource | null = null;
		let audioPassthrough: EncodedAudioPacketSource | null = null;
		if (audioLane && audioTrack) {
			aOut = new Output({
				format: new Mp4OutputFormat({
					fastStart: 'fragmented',
					minimumFragmentDuration: FRAGMENT_S
				}),
				target: new StreamTarget(sinkFor(audioLane, gen))
			});
			if (audioNeedsTranscode && encoderChoice) {
				audioSrc = transcodeSampleSource(encoderChoice, await audioTrack.getNumberOfChannels());
				if (stale()) return bail();
				aOut.addAudioTrack(audioSrc);
			} else {
				const codec = await audioTrack.getCodec();
				if (stale()) return bail();
				if (codec) {
					audioPassthrough = new EncodedAudioPacketSource(codec);
					aOut.addAudioTrack(audioPassthrough);
				}
			}
		}
		// Owned from here: the next seek's `cancelPipelines` reaches them.
		videoOutput = vOut;
		audioOutput = aOut;

		await vOut.start();
		if (stale()) return;
		await aOut?.start();
		if (stale()) return;

		if (seekStart > 0) initialSeek.done = true;

		const otherFed = (lane: Lane | null): number => (lane && !lane.ended ? lane.fedMax : Number.POSITIVE_INFINITY);

		/** Furthest media time the worker has actually produced. Before it has
		 *  produced anything, the segment start — so the first fragments are let
		 *  through rather than deadlocking on an empty buffer. */
		const transcodedEnd = (): number => {
			const b = videoLane?.sb.buffered;
			if (!b || b.length === 0) return mediaStart;
			return Math.max(mediaStart, b.end(b.length - 1));
		};

		const videoPump = (async () => {
			let first = true;
			const decoderConfig = await videoTrack.getDecoderConfig();
			if (stale()) return;
			// Mediabunny closes a fragment only on a keyframe (`keyFrameQueuedEverywhere`
			// in its ISOBMFF muxer), so `minimumFragmentDuration` cannot shorten one: on
			// a scene-cut-keyed x265 rip the first fragment spans a whole GOP — measured
			// at 5.5 MB / ~10 s here — and hevc.js emits nothing until it has all of it.
			// That was 12.6 s of pure transcode before the first frame.
			//
			// We hand mediabunny the packets, so we place the boundaries: marking a
			// delta packet `key` makes the muxer close there. Safe in this pipeline
			// because our fMP4 is read by hevc.js alone, never by the browser's HEVC
			// demuxer, and hevc.js keeps decoder state across segments
			// (`processMediaSegmentStreaming`) — a fragment opening mid-GOP is a
			// continuation, not a random access point. Only the FIRST fragment must
			// start on a real keyframe, and it does: `startPacket`.
			let boundaryStep = FORCED_BOUNDARY_START_S;
			let nextBoundary = mediaStart + boundaryStep;
			for await (const packet of packetSink.packets(startPacket)) {
				if (stale()) break;
				// Open-GOP leading pictures decode after the random access point but
				// present before it; their references are not in this segment.
				if (packet.timestamp < mediaStart) continue;
				await gate.wait(
					() =>
						stale() ||
						(packet.timestamp - effectivePlayhead() <= aheadTarget &&
							packet.timestamp - transcodedEnd() <= IN_FLIGHT_CAP_S &&
							packet.timestamp <= otherFed(audioLane) + TRACK_LEAD_CAP_S)
				);
				if (stale()) break;
				let toAdd = packet;
				if (!first && packet.type !== 'key' && packet.timestamp >= nextBoundary) {
					toAdd = new EncodedPacket(packet.data, 'key', packet.timestamp, packet.duration);
					boundaryStep = Math.min(FORCED_BOUNDARY_MAX_S, boundaryStep * 2);
					nextBoundary = packet.timestamp + boundaryStep;
				}
				await videoSrc.add(toAdd, first ? { decoderConfig: decoderConfig ?? undefined } : undefined);
				if (stale()) break;
				if (first) {
					console.log(`[iris-core] Tier E: first video packet fed at ${packet.timestamp.toFixed(1)}s (gen ${gen})`);
				}
				first = false;
				if (videoLane && packet.timestamp > videoLane.fedMax) videoLane.fedMax = packet.timestamp;
				gate.notify();
			}
			await videoSrc.close();
			if (stale()) return;
			if (videoLane) videoLane.ended = true;
			gate.notify();
		})();

		const audioPump = (async () => {
			if (!audioLane || !audioTrack || (!audioSrc && !audioPassthrough)) return;
			const lane = audioLane;
			const ready = (ts: number) => () =>
				stale() || (ts - effectivePlayhead() <= aheadTarget && ts <= otherFed(videoLane) + TRACK_LEAD_CAP_S);
			if (audioSrc) {
				const sink = new AudioSampleSink(audioTrack);
				for await (const sample of sink.samples(mediaStart, Infinity)) {
					if (stale()) {
						sample.close();
						break;
					}
					await gate.wait(ready(sample.timestamp));
					if (stale()) {
						sample.close();
						break;
					}
					await audioSrc.add(sample);
					sample.close();
					if (stale()) break;
					if (sample.timestamp > lane.fedMax) lane.fedMax = sample.timestamp;
					gate.notify();
				}
				await audioSrc.close();
			} else if (audioPassthrough) {
				const aSink = new EncodedPacketSink(audioTrack);
				const aStart = (await aSink.getKeyPacket(mediaStart)) ?? (await aSink.getFirstKeyPacket());
				if (aStart && !stale()) {
					let first = true;
					const cfg = await audioTrack.getDecoderConfig();
					for await (const packet of aSink.packets(aStart)) {
						if (stale()) break;
						await gate.wait(ready(packet.timestamp));
						if (stale()) break;
						await audioPassthrough.add(packet, first ? { decoderConfig: cfg ?? undefined } : undefined);
						if (stale()) break;
						first = false;
						if (packet.timestamp > lane.fedMax) lane.fedMax = packet.timestamp;
						gate.notify();
					}
				}
				await audioPassthrough.close();
			}
			if (stale()) return;
			lane.ended = true;
			gate.notify();
		})();

		void Promise.all([videoPump, audioPump]).catch((e: unknown) => {
			if (stale()) return;
			fail(e instanceof Error ? e : new Error(String(e)));
		});
	};

	try {
		await ensureIntercept();
		holdsIntercept = true;

		container.appendChild(video);
		// The hold's own pause is not the viewer's: it must neither be saved as one nor turn
		// the chrome's button into "Play" while the transcoder builds its cushion.
		unbindVideo = bindVideoCallbacks(
			video,
			{
				...opts,
				onBusyChange: undefined,
				onPause: (t) => {
					if (!heldPaused) opts.onPause?.(t);
				},
				onPlayingChange: (playing) => {
					if (playing || !heldPaused) opts.onPlayingChange?.(playing);
				}
			},
			initialSeek
		);

		const ms = new MediaSource();
		mediaSource = ms;
		objectUrl = URL.createObjectURL(ms);
		video.src = objectUrl;
		setBusy(true);

		await openMediaSource(ms, 'Tier E');

		if (manifest.duration_s && manifest.duration_s > 0) {
			try {
				ms.duration = manifest.duration_s;
			} catch {
				/* some engines refuse before a buffer exists */
			}
		}

		// Video first: the intercept swaps this one for its transcoding proxy.
		videoLane = makeLane(ms, 'video', `video/mp4; codecs="${videoCodecString}"`);
		if (chosenAudio && audioMp4Codec) {
			audioLane = makeLane(ms, 'audio', `audio/mp4; codecs="${audioMp4Codec}"`);
		}
		console.log(
			`[iris-core] Tier E: video SourceBuffer "${videoCodecString}" (proxied), audio ${audioLane ? `"${audioMp4Codec}"` : 'none'}`
		);

		unsubscribeSpeed = subscribeSegmentStat?.((stat) => {
			if (disposed || !Number.isFinite(stat.speedX) || stat.speedX <= 0) return;
			speeds.push(stat.speedX);
			if (speeds.length > SPEED_WINDOW) speeds.shift();
			const avg = speeds.reduce((a, b) => a + b, 0) / speeds.length;
			const headroom = Math.min(1, Math.max(0.25, avg - 1));
			aheadTarget = Math.min(AHEAD_MAX_S, Math.max(AHEAD_MIN_S, AHEAD_MIN_S / headroom));
			gate.notify();
			// Below real time the cushion drains no matter how deep it is. Say so once
			// per 10 s of playback — it is the difference between "this machine cannot
			// do it" and a transient we already absorbed.
			if (avg < 1 && speeds.length >= 4 && video.currentTime - lastSlowLogT > 10) {
				lastSlowLogT = video.currentTime;
				const ahead = videoLane ? aheadInRange(videoLane.sb.buffered, video.currentTime) : 0;
				console.warn(
					`[iris-core] Tier E: transcode below real time — ${avg.toFixed(2)}x ` +
						`over the last ${speeds.length} segments, ${ahead.toFixed(0)}s of cushion left`
				);
			}
		});

		video.addEventListener('waiting', onStarved);
		for (const e of WAKE_EVENTS) video.addEventListener(e, onProgress);

		input = new Input({
			source: irisUrlSource(streamUrl, { cacheBytes: SOURCE_CACHE_BYTES, ...VOD_RETRY }),
			formats: ALL_FORMATS
		});
		await startPipeline(opts.startPosition);
	} catch (e) {
		await dispose();
		throw e instanceof Error ? e : new Error(String(e));
	}

	const base = videoBackedHandle(video, {
		dispose,
		nativeTrackMap,
		fallbackDuration: manifest.duration_s ?? null
	});

	const handle: EngineHandle = {
		...base,
		// Everything the generic video stats cannot see: this tier's own
		// throughput, the runway it sized from it, and whether playback is being
		// held back on purpose rather than starved.
		stats: () => [
			...(base.stats?.() ?? []),
			[
				'transcode',
				speeds.length
					? `${(speeds.reduce((a, b) => a + b, 0) / speeds.length).toFixed(2)}x real time ` +
						`over ${speeds.length} segment${speeds.length > 1 ? 's' : ''}`
					: 'no segment measured yet'
			],
			['cushion', `${cushion().toFixed(1)}s of ${aheadTarget.toFixed(0)}s runway`],
			[
				'gate',
				holdUntil > 0
					? `holding for ${holdUntil}s of cushion`
					: videoLane
						? `feeding, fed to ${videoLane.fedMax.toFixed(1)}s`
						: 'no video lane'
			],
			['pipeline', `generation ${generation}, ${videoCodecString} proxied to H.264`]
		],
		// During a hold, play means "start as soon as there is enough", not "start
		// now" — starting now is exactly what stutters. Pause cancels that intent.
		play: async () => {
			if (holdUntil > 0) {
				heldPaused = true;
				enforceHold();
				return;
			}
			await video.play();
		},
		pause: () => {
			heldPaused = false;
			video.pause();
		},
		// Held for a cushion with the intent to play: playing, as far as the viewer is told.
		paused: () => !heldPaused && video.paused,
		// While a restart is in flight the element still sits at the old position;
		// report where we are heading instead.
		currentTime: () => effectivePlayhead(),
		seek: (s: number) => {
			const target = Math.max(0, s);
			if (videoLane && coversTime(videoLane.sb.buffered, target)) {
				pendingAnchor = null;
				try {
					video.currentTime = target;
				} catch {
					/* swallow */
				}
				return;
			}
			void startPipeline(target).catch((e: unknown) => {
				console.warn('[iris-core] Tier E: seek pipeline failed', e);
			});
		}
	};
	return handle;
};
