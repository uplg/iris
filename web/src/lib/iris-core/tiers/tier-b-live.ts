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
  AudioSampleSource,
  EncodedAudioPacketSource,
  EncodedPacketSink,
  EncodedVideoPacketSource,
  Input,
  Mp4OutputFormat,
  Output,
  Quality,
  StreamTarget,
  type StreamTargetChunk,
  UrlSource,
} from "mediabunny";

import { ensureLibavAudioDecoderRegistered, libavCanDecode } from "../decode/libav-audio-decoder";
import {
  bindVideoCallbacks,
  videoBackedHandle,
  type EngineHandle,
  type EngineMount,
} from "../engine";
import { pickAudioEncoder, relaxMediabunnyGopCheck } from "./tier-b-mse";

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
/** In-place pipeline restarts allowed within the rolling window before the
 *  error surfaces to the page (which then rotates sources). */
const RESTART_BUDGET = 3;
const RESTART_WINDOW_MS = 90_000;
/** Consecutive landed appends with a frozen playhead (while un-paused with
 *  buffer ahead) before we call the decoder wedged and restart. ~16 appends
 *  ≈ several seconds of media landing with zero playback progress. */
const WEDGE_APPEND_LIMIT = 16;

/** Codecs MSE plays inside fMP4 without help — passthrough, no re-encode. */
const MSE_NATIVE_AUDIO = new Set(["aac", "opus", "mp3"]);
/** Key packets to walk while hunting a true IDR anchor. At broadcast IDR
 *  cadence (~1-4 s) this covers the whole live window and then some. */
const IDR_HUNT_LIMIT = 24;

function isFirefox(): boolean {
  return typeof navigator !== "undefined" && /Firefox\/\d+/.test(navigator.userAgent);
}

/** NAL length-prefix size from the avcC description (defaults to 4). */
function nalLengthSize(description: BufferSource | undefined): number {
  if (!description) return 4;
  const bytes =
    description instanceof ArrayBuffer
      ? new Uint8Array(description)
      : new Uint8Array(description.buffer, description.byteOffset, description.byteLength);
  // avcC: [0]=version [1]=profile [2]=compat [3]=level [4]=0xFC|lengthSizeMinusOne
  if (bytes.length < 5 || bytes[0] !== 1) return 4;
  return (bytes[4]! & 0x03) + 1;
}

/** True when the AVCC sample contains an IDR slice (NAL type 5). Broadcast
 *  TNT mostly emits open-GOP recovery-point I-frames — the fMP4 marks them
 *  as sync samples, but starting a strict decoder (VideoToolbox) on one is
 *  an illegal random access and it hard-fails instantly. Only a real IDR
 *  (fresh DPB) is a safe anchor. */
function packetHasIdr(data: Uint8Array, lengthSize: number): boolean {
  let o = 0;
  while (o + lengthSize < data.length) {
    let len = 0;
    for (let i = 0; i < lengthSize; i += 1) len = (len << 8) | data[o + i]!;
    o += lengthSize;
    if (len <= 0 || o + len > data.length) break;
    if ((data[o]! & 0x1f) === 5) return true;
    o += len;
  }
  return false;
}

export const mountTierBLive: EngineMount = async (opts) => {
  const { container, streamUrl } = opts;
  const fail = (err: Error) => opts.onError(err);

  if (typeof globalThis.MediaSource === "undefined") {
    const err = new Error("MediaSource Extensions not available");
    fail(err);
    throw err;
  }

  container.innerHTML = "";
  const video = document.createElement("video");
  video.className = "h-full w-full object-contain";
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
  const appendQueue: Uint8Array[] = [];
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

  // ---- waiters (all flushed on dispose AND on restart) --------------

  const trackWaiters = new Set<() => void>();
  const notifyTrackProgress = () => {
    for (const w of trackWaiters) w();
  };
  const bufferRoomWaiters = new Set<() => void>();
  const notifyBufferRoom = () => {
    for (const w of bufferRoomWaiters) w();
  };
  const sinkWaiters = new Set<() => void>();
  const flushAllWaiters = () => {
    for (const w of trackWaiters) w();
    trackWaiters.clear();
    for (const w of bufferRoomWaiters) w();
    bufferRoomWaiters.clear();
    for (const w of sinkWaiters) w();
    sinkWaiters.clear();
  };

  const waitTrackBalance = (gen: number, ts: number, otherFedMax: () => number): Promise<void> =>
    new Promise<void>((resolve) => {
      const ready = () => disposed || gen !== generation || ts <= otherFedMax() + TRACK_LEAD_CAP;
      if (ready()) {
        resolve();
        return;
      }
      const w = () => {
        if (!ready()) return;
        trackWaiters.delete(w);
        resolve();
      };
      trackWaiters.add(w);
    });

  const waitBufferRoom = (gen: number, ts: number): Promise<void> =>
    new Promise<void>((resolve) => {
      const ready = () =>
        disposed || gen !== generation || ts - video.currentTime <= AHEAD_TARGET_S;
      if (ready()) {
        resolve();
        return;
      }
      const w = () => {
        if (!ready()) return;
        bufferRoomWaiters.delete(w);
        resolve();
      };
      bufferRoomWaiters.add(w);
    });

  // ---- buffer plumbing ------------------------------------------------

  const bufferedAheadSeconds = (): number => {
    if (!sourceBuffer || sourceBuffer.buffered.length === 0) return 0;
    const t = video.currentTime;
    const b = sourceBuffer.buffered;
    let coveredEnd = Number.NEGATIVE_INFINITY;
    for (let i = 0; i < b.length; i += 1) {
      const start = b.start(i);
      const end = b.end(i);
      if (coveredEnd === Number.NEGATIVE_INFINITY) {
        if (start <= t + 0.5 && end >= t) coveredEnd = end;
      } else if (start - coveredEnd <= 2) {
        coveredEnd = end; // bridge non-coalescing fMP4 fragment ranges
      } else {
        break;
      }
    }
    return coveredEnd === Number.NEGATIVE_INFINITY ? 0 : Math.max(0, coveredEnd - t);
  };

  const evictPlayedRange = (): void => {
    // Firefox: never run our own remove() — it can wedge `updating=true`
    // forever (VOD Tier B lore); FF's native eviction handles the shallow
    // live window fine.
    if (firefox || !sourceBuffer || sourceBuffer.updating) return;
    const evictBefore = video.currentTime - BEHIND_KEEP_S;
    if (evictBefore <= 0 || sourceBuffer.buffered.length === 0) return;
    const firstStart = sourceBuffer.buffered.start(0);
    if (firstStart >= evictBefore) return;
    try {
      sourceBuffer.remove(firstStart, evictBefore);
    } catch {
      /* retried on the next tick */
    }
  };

  const drainQueue = () => {
    if (disposed || !sourceBuffer || sourceBuffer.updating) return;
    const next = appendQueue.shift();
    if (!next) return;
    try {
      sourceBuffer.appendBuffer(next.slice().buffer);
    } catch (e) {
      if (e instanceof DOMException && e.name === "QuotaExceededError") {
        appendQueue.unshift(next);
        evictPlayedRange();
        return;
      }
      fail(e instanceof Error ? e : new Error(String(e)));
    }
  };

  /** Jump the playhead across a small forward gap. True when it moved. */
  const jumpForwardGap = (): boolean => {
    if (!sourceBuffer) return false;
    const t = video.currentTime;
    for (let i = 0; i < sourceBuffer.buffered.length; i += 1) {
      const start = sourceBuffer.buffered.start(i);
      const end = sourceBuffer.buffered.end(i);
      if (end - start < 0.05) continue;
      if (start > t && start - t < 8) {
        console.warn(`[iris-core] live: jumping gap ${t.toFixed(2)} → ${start.toFixed(2)}`);
        try {
          video.currentTime = start + 0.01;
        } catch {
          /* swallow */
        }
        return true;
      }
    }
    return false;
  };

  // ---- in-place restart ------------------------------------------------

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
      fail(
        new Error(
          `live: pipeline restarted ${RESTART_BUDGET}x in ${RESTART_WINDOW_MS / 1000}s — giving up (${reason})`,
        ),
      );
      return;
    }
    restartStamps.push(now);
    const wasPlaying = !video.paused;
    console.warn(
      `[iris-core] live: in-place pipeline restart #${restartStamps.length} (${reason}) ` +
        `t=${video.currentTime.toFixed(1)} playing=${wasPlaying}`,
    );
    generation += 1;
    flushAllWaiters();
    appendQueue.length = 0;
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

  // ---- video element error handling -------------------------------------

  // Per-cycle one-shot (Firefox can fire `error` repeatedly). Reset by
  // `startCycle` when it re-arms the element with a fresh MediaSource.
  let elementErrorHandled = false;
  const onErr = () => {
    if (disposed || elementErrorHandled) return;
    elementErrorHandled = true;
    const err = video.error;
    console.warn(
      `[iris-core] live: media element error code=${err?.code} t=${video.currentTime.toFixed(1)} ` +
        `msg=${err?.message ?? ""}`,
    );
    // MEDIA_ERR_DECODE (3): a platform-decoder trip on a rough broadcast
    // GOP — recover in place with a fresh decoder session. Anything else
    // (src not supported, network on a blob…) is structural: surface it.
    if (err?.code === 3) {
      void restartPipeline("media decode error");
    } else {
      fail(new Error(err ? `media error ${err.code}: ${err.message}` : "video element error"));
    }
  };
  video.addEventListener("error", onErr);

  // Playhead advanced → trim behind, retry queued appends, release feeds.
  const onTimeUpdate = () => {
    if (disposed) return;
    evictPlayedRange();
    if (appendQueue.length > 0) drainQueue();
    notifyBufferRoom();
  };
  video.addEventListener("timeupdate", onTimeUpdate);

  // Underrun: unwedge a hung append (FF bug 1120084 lore), jump a gap.
  const onWaiting = () => {
    if (disposed || !sourceBuffer) return;
    if (sourceBuffer.updating) {
      try {
        sourceBuffer.abort();
        console.warn("[iris-core] live: aborted wedged SourceBuffer op (FF unwedge)");
      } catch {
        /* MediaSource not open — dispose path owns it */
      }
    }
    drainQueue();
    jumpForwardGap();
  };
  video.addEventListener("waiting", onWaiting);
  video.addEventListener("stalled", onWaiting);

  // ---- dispose -----------------------------------------------------------

  const dispose = async (): Promise<void> => {
    if (disposed) return;
    disposed = true;
    generation += 1;
    flushAllWaiters();
    unbindVideo();
    video.removeEventListener("error", onErr);
    video.removeEventListener("waiting", onWaiting);
    video.removeEventListener("stalled", onWaiting);
    video.removeEventListener("timeupdate", onTimeUpdate);
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
    if (objectUrl) URL.revokeObjectURL(objectUrl);
    try {
      if (mediaSource && mediaSource.readyState === "open") mediaSource.endOfStream();
    } catch {
      /* idempotent */
    }
    try {
      video.pause();
    } catch {
      /* idempotent */
    }
  };

  // ---- stream probing (once per mount) ------------------------------------

  let mime = "";
  let videoDecoderConfigCodec = "";
  type AudioPlan =
    | { kind: "passthrough"; mp4Codec: string }
    | { kind: "transcode"; mp4Codec: string; targetCodec: "aac" | "opus"; channels: number }
    | null;
  let audioPlan: AudioPlan = null;

  /** (Re)create the MediaSource + SourceBuffer on the `<video>`, wire the
   *  per-cycle listeners, then anchor at the live edge and spawn the feed
   *  loops. Used by the initial mount and by every in-place restart. */
  const startCycle = async (resumePlaying: boolean): Promise<void> => {
    const gen = generation;
    const liveInput = input;
    if (!liveInput) throw new Error("live: startCycle before input init");
    // Phase timing — tells us exactly where a slow start went.
    const t0 = performance.now();
    const phase = (label: string) =>
      console.log(
        `[iris-core] live cycle #${gen} phase: ${label} +${(performance.now() - t0).toFixed(0)}ms`,
      );

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

    await new Promise<void>((resolve, reject) => {
      const onOpen = () => {
        ms.removeEventListener("sourceopen", onOpen);
        ms.removeEventListener("error", onMseErr);
        resolve();
      };
      const onMseErr = () => {
        ms.removeEventListener("sourceopen", onOpen);
        ms.removeEventListener("error", onMseErr);
        reject(new Error("MediaSource emitted error before opening"));
      };
      ms.addEventListener("sourceopen", onOpen);
      ms.addEventListener("error", onMseErr);
    });
    if (disposed || gen !== generation) return;

    const sb = ms.addSourceBuffer(mime);
    sb.mode = "segments";
    sourceBuffer = sb;
    sb.addEventListener("updateend", () => {
      if (disposed || gen !== generation) return;
      drainQueue();
      if (sb.buffered.length === 0) return;
      if (!playbackStarted) {
        // First media landed → anchor the playhead + (re)start playback.
        playbackStarted = true;
        const start = sb.buffered.start(0);
        console.log(
          `[iris-core] live: first media buffered [${start.toFixed(2)}-${sb.buffered
            .end(0)
            .toFixed(2)}] — anchoring playhead`,
        );
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
            console.warn("[iris-core] live: autoplay blocked — press play");
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
            void restartPipeline("decoder wedged (frozen playhead)");
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
    sb.addEventListener("error", () => {
      if (disposed || gen !== generation) return;
      // The element-level error handler decides recover-vs-surface.
      console.warn("[iris-core] live: SourceBuffer error event");
    });

    // ---- anchor at the live edge (playlist metadata, never client clock) --
    phase("MediaSource open");
    const videoTrack = await liveInput.getPrimaryVideoTrack();
    if (!videoTrack) throw new Error("live: no video track");
    const videoPacketSink = new EncodedPacketSink(videoTrack);
    const videoDecoderConfig = await videoTrack.getDecoderConfig();
    const sourceVideoCodec = await videoTrack.getCodec();
    if (!videoDecoderConfig || !sourceVideoCodec) throw new Error("live: video codec unknown");
    phase("video track probed");
    // skipLiveWait is load-bearing: without it this resolves only once the
    // live stream ENDS (it follows the growing playlist forever).
    const windowEnd =
      (await liveInput.getDurationFromMetadata(undefined, { skipLiveWait: true })) ?? 0;
    const edgeTarget = Math.max(0, windowEnd - LIVE_EDGE_BACKOFF_S, anchorFloor);
    let startPacket = await videoPacketSink.getKeyPacket(edgeTarget);
    if (!startPacket) startPacket = await videoPacketSink.getFirstKeyPacket();
    if (!startPacket) throw new Error("live: no video keyframe in window");
    phase(`edge keyframe found (target=${edgeTarget.toFixed(1)}s)`);
    // Walk forward to a true IDR: fMP4 sync samples include open-GOP
    // recovery points that strict decoders can't start on (see
    // `packetHasIdr`). Each step at the live edge can BLOCK for a whole
    // segment duration (the sink follows the growing playlist) — the
    // per-step log tells us when startup time is going into this hunt.
    const lenSize = nalLengthSize(
      (await videoTrack.getDecoderConfig())?.description as BufferSource | undefined,
    );
    let hunted = 0;
    let idrPacket = startPacket;
    while (hunted < IDR_HUNT_LIMIT && !packetHasIdr(idrPacket.data, lenSize)) {
      if (disposed || gen !== generation) return;
      const stepStart = performance.now();
      const next = await videoPacketSink.getNextKeyPacket(idrPacket);
      const stepMs = performance.now() - stepStart;
      if (stepMs > 500) {
        console.log(
          `[iris-core] live: IDR hunt step ${hunted + 1} blocked ${stepMs.toFixed(0)}ms ` +
            `(waiting on live segments)`,
        );
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
          `anchoring on a recovery point (strict decoders may object)`,
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
        `idrHunt=${hunted}`,
    );

    // ---- output + sink ----------------------------------------------------
    let sinkChunks = 0;
    const newOutput = new Output({
      format: new Mp4OutputFormat({ fastStart: "fragmented", minimumFragmentDuration: 1 }),
      target: new StreamTarget(
        new WritableStream<StreamTargetChunk>({
          write: async (chunk) => {
            if (disposed || gen !== generation) return;
            sinkChunks += 1;
            if (sinkChunks <= 2) {
              console.log(
                `[iris-core] live: muxer chunk #${sinkChunks} (${chunk.data.byteLength} bytes)`,
              );
            }
            appendQueue.push(chunk.data);
            drainQueue();
            // Park until an append lands or playback drains the buffer —
            // event-driven; dispose/restart flushes `sinkWaiters`.
            while (
              !disposed &&
              gen === generation &&
              (bufferedAheadSeconds() > AHEAD_TARGET_S || appendQueue.length > MAX_QUEUED_CHUNKS)
            ) {
              await new Promise<void>((resolve) => {
                if (!sourceBuffer) {
                  resolve();
                  return;
                }
                let settled = false;
                const done = () => {
                  if (settled) return;
                  settled = true;
                  sourceBuffer?.removeEventListener("updateend", done);
                  video.removeEventListener("timeupdate", done);
                  sinkWaiters.delete(done);
                  resolve();
                };
                sinkWaiters.add(done);
                sourceBuffer.addEventListener("updateend", done, { once: true });
                video.addEventListener("timeupdate", done, { once: true });
              });
            }
          },
          close: () => {
            // ENDLIST (the backend session died) → let the element end; the
            // page's onEnded handler rotates to the next source.
            if (disposed || gen !== generation) return;
            try {
              if (ms.readyState === "open") ms.endOfStream();
            } catch {
              /* idempotent */
            }
          },
          abort: (reason) => {
            if (disposed || gen !== generation) return;
            fail(reason instanceof Error ? reason : new Error(String(reason)));
          },
        }),
      ),
    });
    relaxMediabunnyGopCheck(newOutput);
    output = newOutput;

    const videoSrc = new EncodedVideoPacketSource(sourceVideoCodec);
    newOutput.addVideoTrack(videoSrc);

    const audioTrack = (await liveInput.getAudioTracks())[0] ?? null;
    type AudioFeed =
      | { kind: "passthrough"; source: EncodedAudioPacketSource }
      | { kind: "transcode"; source: AudioSampleSource };
    let audioFeed: AudioFeed | null = null;
    if (audioTrack && audioPlan) {
      if (audioPlan.kind === "transcode") {
        const srcChannels = await audioTrack.getNumberOfChannels();
        const source = new AudioSampleSource({
          codec: audioPlan.targetCodec,
          // `new Quality(<number>)` means a 0..1 qualitative level, NOT
          // a bitrate — the explicit `{ bitrate }` form is required.
          quality: new Quality({ bitrate: audioPlan.targetCodec === "opus" ? 128_000 : 192_000 }),
          ...(audioPlan.channels !== srcChannels
            ? { transform: { numberOfChannels: audioPlan.channels } }
            : {}),
        });
        newOutput.addAudioTrack(source);
        audioFeed = { kind: "transcode", source };
      } else {
        const sourceAudioCodec = await audioTrack.getCodec();
        if (sourceAudioCodec) {
          const source = new EncodedAudioPacketSource(sourceAudioCodec);
          newOutput.addAudioTrack(source);
          audioFeed = { kind: "passthrough", source };
        }
      }
    }

    await newOutput.start();
    if (disposed || gen !== generation) return;

    // ---- feed loops (anchor-relative timestamps into the muxer) -----------

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
        await videoSrc.add(
          packet.clone({ timestamp: rel }),
          firstMeta ? { decoderConfig: videoDecoderConfig } : undefined,
        );
        firstMeta = false;
        if (rel > videoFedMax) videoFedMax = rel;
        if (rel - lastLogged >= 5) {
          lastLogged = rel;
          const q = video.getVideoPlaybackQuality?.();
          console.log(
            `[iris-core] live: fed v=${videoFedMax.toFixed(1)}s a=${audioFedMax.toFixed(1)}s ` +
              `t=${video.currentTime.toFixed(1)}s ahead=${bufferedAheadSeconds().toFixed(1)}s ` +
              `queue=${appendQueue.length} chunks=${sinkChunks} rs=${video.readyState}` +
              (q ? ` frames=${q.totalVideoFrames}/drop=${q.droppedVideoFrames}` : ""),
          );
        }
        notifyTrackProgress();
      }
      try {
        await videoSrc.close();
      } catch {
        /* output cancelled mid-flush — teardown noise */
      }
      videoFedMax = Number.POSITIVE_INFINITY;
      notifyTrackProgress();
    })();

    if (!(audioTrack && audioFeed)) {
      audioFedMax = Number.POSITIVE_INFINITY;
      notifyTrackProgress();
    }
    const audioP =
      audioTrack && audioFeed
        ? (async () => {
            const feed = audioFeed;
            if (feed.kind === "passthrough") {
              const packetSink = new EncodedPacketSink(audioTrack);
              let start = await packetSink.getKeyPacket(anchor);
              if (!start) start = await packetSink.getFirstKeyPacket();
              if (!start) {
                try {
                  try {
                    await feed.source.close();
                  } catch {
                    /* output cancelled mid-flush — teardown noise */
                  }
                } catch {
                  /* output cancelled mid-flush — teardown noise */
                }
                audioFedMax = Number.POSITIVE_INFINITY;
                notifyTrackProgress();
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
                  firstMeta ? { decoderConfig: decoderConfig ?? undefined } : undefined,
                );
                firstMeta = false;
                if (rel > audioFedMax) audioFedMax = rel;
                notifyTrackProgress();
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
                  if (rel > audioFedMax) audioFedMax = rel;
                  notifyTrackProgress();
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
            audioFedMax = Number.POSITIVE_INFINITY;
            notifyTrackProgress();
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

  // ---- mount: probe once, then run the first cycle -----------------------

  const mount0 = performance.now();
  try {
    input = new Input({
      source: new UrlSource(streamUrl, {
        // 5xx → transient: reject so mediabunny's retry kicks in (a source
        // rotation briefly 502s while the backend elects the next feed).
        fetchFn: async (fetchInput, init) => {
          const res = await fetch(fetchInput, init);
          if (res.status >= 500) {
            throw new Error(`iris-live-transient-5xx ${res.status}`);
          }
          return res;
        },
        getRetryDelay: (attempts) => (attempts >= 6 ? null : Math.min(4, 0.5 * 2 ** attempts)),
        maxCacheSize: 32 * 1024 * 1024,
      }),
      formats: ALL_FORMATS,
      // Gapless continuous timeline — see the module header. Wall-clock
      // times stay reachable via `InputTrack.getUnixTimeForTimestamp`.
      formatOptions: { hls: { offsetTimestampsByDateTime: false } },
    });

    const videoTrack = await input.getPrimaryVideoTrack();
    if (!videoTrack) throw new Error("live: no video track in stream");
    const videoDecoderConfig = await videoTrack.getDecoderConfig();
    if (!videoDecoderConfig?.codec) throw new Error("live: video codec unknown");
    videoDecoderConfigCodec = videoDecoderConfig.codec;

    const audioTrack = (await input.getAudioTracks())[0] ?? null;
    const audioCodec = audioTrack ? await audioTrack.getCodec() : null;
    if (audioTrack && audioCodec) {
      if (MSE_NATIVE_AUDIO.has(audioCodec)) {
        const cfg = await audioTrack.getDecoderConfig();
        audioPlan = { kind: "passthrough", mp4Codec: cfg?.codec ?? "mp4a.40.2" };
      } else if (libavCanDecode(audioCodec)) {
        ensureLibavAudioDecoderRegistered();
        const channels = await audioTrack.getNumberOfChannels();
        const sampleRate = await audioTrack.getSampleRate();
        const choice = await pickAudioEncoder(channels, sampleRate);
        if (!choice) throw new Error(`live: cannot re-encode ${audioCodec} in this browser`);
        audioPlan = {
          kind: "transcode",
          mp4Codec: choice.mp4Codec,
          targetCodec: choice.codec,
          channels: choice.channels,
        };
      } else {
        console.warn(`[iris-core] live: audio codec ${audioCodec} undecodable — video only`);
      }
    }

    const codecs = [videoDecoderConfigCodec, audioPlan?.mp4Codec].filter(Boolean).join(",");
    mime = `video/mp4; codecs="${codecs}"`;
    if (!MediaSource.isTypeSupported(mime)) {
      throw new Error(`live: MIME not supported by MSE: ${mime}`);
    }
    console.log(
      `[iris-core] live mount: video=${videoDecoderConfigCodec} audio=${audioCodec ?? "none"}` +
        `${audioPlan ? ` (${audioPlan.kind} → ${audioPlan.mp4Codec})` : ""} ` +
        `probeMs=${(performance.now() - mount0).toFixed(0)}`,
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
    audioTracks: () => [],
  });
  return handle;
};
