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
  AudioSampleSource,
  EncodedPacket,
  EncodedPacketSink,
  EncodedVideoPacketSource,
  Input,
  Mp4OutputFormat,
  Output,
  Quality,
  type StreamTargetChunk,
  StreamTarget,
  UrlSource,
} from "mediabunny";

import {
  appendNativeTrack,
  bindVideoCallbacks,
  videoBackedHandle,
  type EngineHandle,
  type EngineMount,
} from "../engine";
import { ensureLibavAudioDecoderRegistered, libavCanDecode } from "../decode/libav-audio-decoder";
import { pickAudioEncoder, relaxMediabunnyGopCheck } from "./tier-b-mse";

/** `subscribeSegmentStat` from `@hevcjs/core`, captured on first load. The lib
 *  publishes one stat per transcoded segment, `speedX` being media-seconds
 *  produced per wall-second — the only honest measure of whether the WASM
 *  decoder is keeping up on this machine right now. */
let subscribeSegmentStat: ((l: (s: { speedX: number }) => void) => () => void) | null = null;

const WASM_URL = "/hevcjs/hevc-decode.js";
const WASM_BINARY_URL = "/hevcjs/hevc-decode.wasm";
const WORKER_URL = "/hevcjs/transcode-worker.js";

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
 *  and parks tens of seconds of compressed HEVC in its queue — memory pressure
 *  that slows the very transcode it is waiting on. The cushion has to be built
 *  out of transcoded output, not backlog. */
const IN_FLIGHT_CAP_S = 8;
/** Rolling window over per-segment throughput, in segments. At the forced
 *  boundary cadence that is roughly the last 12 s of media. */
const SPEED_WINDOW = 8;

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
  if (typeof g.HEVCDecoderModule === "function") return Promise.resolve();
  const existing = document.querySelector<HTMLScriptElement>(`script[src="${WASM_URL}"]`);
  const el = existing ?? document.createElement("script");
  const done = new Promise<void>((resolve, reject) => {
    el.addEventListener("load", () => resolve(), { once: true });
    el.addEventListener("error", () => reject(new Error(`Tier E: failed to load ${WASM_URL}`)), {
      once: true,
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
    const mod = await import("@hevcjs/core");
    subscribeSegmentStat = mod.subscribeSegmentStat;
    await ensureDecoderGlobal();
    intercept = {
      install: () =>
        mod.installMSEIntercept({
          wasmUrl: WASM_URL,
          wasmBinaryUrl: WASM_BINARY_URL,
          workerUrl: WORKER_URL,
          logLevel: "warn",
        }),
      uninstall: () => mod.uninstallMSEIntercept(),
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
    const end = InitFramer.initSegmentEnd(joined);
    if (end < 0) return []; // `moov` not complete yet — keep accumulating
    this.initDone = true;
    this.pending = [];
    this.pendingBytes = 0;
    const rest = joined.subarray(end);
    return rest.byteLength > 0 ? [joined.subarray(0, end), rest] : [joined.subarray(0, end)];
  }

  /** Byte offset just past `moov`, or -1 while it is still incomplete. */
  private static initSegmentEnd(buf: Uint8Array): number {
    if (buf.byteLength < 8) return -1;
    const dv = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    let o = 0;
    while (o + 8 <= buf.byteLength) {
      let size = dv.getUint32(o);
      const type = String.fromCharCode(buf[o + 4]!, buf[o + 5]!, buf[o + 6]!, buf[o + 7]!);
      if (size === 1) {
        if (o + 16 > buf.byteLength) return -1;
        size = Number(dv.getBigUint64(o + 8));
      }
      if (size < 8) return -1;
      if (o + size > buf.byteLength) return -1; // box truncated
      if (type === "moov") return o + size;
      o += size;
    }
    return -1;
  }
}

/** One SourceBuffer plus the serialised queue feeding it. */
type Lane = {
  name: "video" | "audio";
  sb: SourceBuffer;
  queue: Uint8Array[];
  draining: boolean;
  fedMax: number;
  ended: boolean;
  /** Diagnostics: how many appends landed, and whether we logged first data. */
  appended: number;
  reported: boolean;
};

export const mountTierE: EngineMount = async (opts) => {
  const { container, manifest, streamUrl, nativeSubs, audioTrackIndex } = opts;

  if (typeof globalThis.MediaSource === "undefined") {
    throw new Error("Tier E: MediaSource is not available");
  }
  const videoCodecString = manifest.video[0]?.codec_string;
  if (!videoCodecString) throw new Error("Tier E: manifest has no video codec string");

  const chosenAudioIdx = Math.max(0, audioTrackIndex ?? manifest.audio.findIndex((a) => a.default));
  const chosenAudio = manifest.audio[chosenAudioIdx] ?? null;
  const audioNeedsTranscode = chosenAudio != null && !chosenAudio.browser_native;
  if (audioNeedsTranscode && !libavCanDecode(chosenAudio.codec)) {
    throw new Error(`Tier E: audio codec ${chosenAudio.codec} not transcodable client-side`);
  }
  if (audioNeedsTranscode) ensureLibavAudioDecoderRegistered();

  let encoderChoice: Awaited<ReturnType<typeof pickAudioEncoder>> = null;
  if (audioNeedsTranscode && chosenAudio) {
    encoderChoice = await pickAudioEncoder(chosenAudio.channels, chosenAudio.sample_rate ?? 48000);
    if (!encoderChoice) {
      throw new Error("Tier E: no usable AudioEncoder for this source");
    }
  }
  const audioMp4Codec = audioNeedsTranscode
    ? (encoderChoice?.mp4Codec ?? "mp4a.40.2")
    : chosenAudio?.codec_string;

  await ensureIntercept();

  container.innerHTML = "";
  const video = document.createElement("video");
  video.className = "h-full w-full object-contain";
  video.playsInline = true;
  const nativeTrackMap = new Map<number, HTMLTrackElement>();
  for (const sub of nativeSubs) appendNativeTrack(video, sub, nativeTrackMap);
  container.appendChild(video);

  const initialSeek = { done: false };
  const unbindVideo = bindVideoCallbacks(video, opts, initialSeek);

  const mediaSource = new MediaSource();
  const objectUrl = URL.createObjectURL(mediaSource);
  video.src = objectUrl;

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

  const fail = (e: Error) => {
    if (disposed) return;
    opts.onError?.(e);
  };

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
  setBusy(true);

  await new Promise<void>((resolve, reject) => {
    const onOpen = () => {
      mediaSource.removeEventListener("sourceopen", onOpen);
      resolve();
    };
    const onErr = () => {
      mediaSource.removeEventListener("sourceopen", onOpen);
      reject(new Error("Tier E: MediaSource errored before opening"));
    };
    mediaSource.addEventListener("sourceopen", onOpen, { once: true });
    mediaSource.addEventListener("error", onErr, { once: true });
  });

  if (manifest.duration_s && manifest.duration_s > 0) {
    try {
      mediaSource.duration = manifest.duration_s;
    } catch {
      /* some engines refuse before a buffer exists */
    }
  }

  const makeLane = (name: "video" | "audio", mime: string): Lane => {
    const sb = mediaSource.addSourceBuffer(mime);
    sb.mode = "segments";
    return {
      name,
      sb,
      queue: [],
      draining: false,
      fedMax: 0,
      ended: false,
      appended: 0,
      reported: false,
    };
  };

  // Video first: the intercept swaps this one for its transcoding proxy.
  videoLane = makeLane("video", `video/mp4; codecs="${videoCodecString}"`);
  if (chosenAudio && audioMp4Codec) {
    audioLane = makeLane("audio", `audio/mp4; codecs="${audioMp4Codec}"`);
  }
  console.log(
    `[iris-core] Tier E: video SourceBuffer "${videoCodecString}" (proxied), ` +
      `audio ${audioLane ? `"${audioMp4Codec}"` : "none"}`,
  );

  const effectivePlayhead = (): number => pendingAnchor ?? video.currentTime;

  const applyPendingAnchor = (): void => {
    const t = pendingAnchor;
    if (t === null || !videoLane) return;
    const b = videoLane.sb.buffered;
    for (let i = 0; i < b.length; i += 1) {
      if (b.start(i) - 0.25 <= t && b.end(i) >= t) {
        pendingAnchor = null;
        try {
          if (Math.abs(video.currentTime - t) > 0.05) video.currentTime = t;
        } catch {
          /* swallow */
        }
        return;
      }
    }
  };

  /** Append one buffer and resolve when the SourceBuffer is idle again.
   *
   *  The hevc.js proxy fires `updateend` as soon as it has *queued* the data,
   *  not when the transcode lands, and its `updating` stays false throughout —
   *  so this only serialises our own calls, which is all `appendBuffer`
   *  requires. The transcode back-pressure comes from the feed gates instead. */
  const appendOnce = (lane: Lane, data: Uint8Array): Promise<void> =>
    new Promise<void>((resolve) => {
      let settled = false;
      const done = () => {
        if (settled) return;
        settled = true;
        lane.sb.removeEventListener("updateend", done);
        lane.sb.removeEventListener("error", done);
        lane.sb.removeEventListener("abort", done);
        resolve();
      };
      lane.sb.addEventListener("updateend", done, { once: true });
      lane.sb.addEventListener("error", done, { once: true });
      // A restart calls `abort()` to flush the proxy's backlog. The spec queues
      // `updateend` after `abort`, but this SourceBuffer is hevc.js's stand-in
      // — settle on either, or a drain loop parks forever holding `draining`.
      lane.sb.addEventListener("abort", done, { once: true });
      try {
        // hevc.js transfers the buffer to its worker; a view onto a shared
        // ArrayBuffer would detach the parent and starve every later append.
        lane.sb.appendBuffer(data.slice().buffer);
      } catch (e) {
        if (e instanceof DOMException && e.name === "QuotaExceededError") {
          lane.queue.unshift(data);
        }
        done();
      }
    });

  const drain = async (lane: Lane): Promise<void> => {
    if (lane.draining) return;
    lane.draining = true;
    try {
      while (!disposed && lane.queue.length > 0) {
        const next = lane.queue.shift();
        if (!next) break;
        await appendOnce(lane, next);
        lane.appended += 1;
        if (!lane.reported && lane.sb.buffered.length > 0) {
          lane.reported = true;
          if (lane.name === "video") setBusy(false);
          const b = lane.sb.buffered;
          console.log(
            `[iris-core] Tier E: ${lane.name} lane first buffered ` +
              `[${b.start(0).toFixed(1)}-${b.end(0).toFixed(1)}] after ${lane.appended} appends`,
          );
        }
        applyPendingAnchor();
      }
    } finally {
      lane.draining = false;
    }
  };

  const sinkFor = (lane: Lane, gen: number): WritableStream<StreamTargetChunk> => {
    // Only the video lane goes through the hevc.js proxy, and only it needs the
    // init segment delivered whole. The audio lane is a plain SourceBuffer and
    // takes mediabunny's chunking as-is.
    const framer = lane.name === "video" ? new InitFramer() : null;
    return new WritableStream<StreamTargetChunk>({
      write: (chunk) => {
        if (disposed || gen !== generation) return;
        const parts = framer ? framer.push(chunk.data) : [chunk.data];
        for (const part of parts) lane.queue.push(part);
        if (parts.length > 0) void drain(lane);
      },
    });
  };

  /** Contiguous seconds buffered ahead of `t`, 0 if `t` isn't in a range. */
  const bufferedAheadOf = (b: TimeRanges, t: number): number => {
    for (let i = 0; i < b.length; i += 1) {
      if (b.start(i) - 0.25 <= t && b.end(i) + 0.25 >= t) return Math.max(0, b.end(i) - t);
    }
    return 0;
  };

  const waiters = new Set<() => void>();
  const notify = () => {
    // Copy first: a waiter that resolves removes itself from the set.
    for (const w of Array.from(waiters)) w();
  };
  const gate = (ready: () => boolean): Promise<void> =>
    new Promise<void>((resolve) => {
      if (ready()) return resolve();
      const w = () => {
        if (!ready()) return;
        waiters.delete(w);
        resolve();
      };
      waiters.add(w);
    });

  // Throughput-sized runway. `speedX` is media-seconds transcoded per
  // wall-second; `speedX - 1` is the rate at which the cushion grows during
  // playback. The thinner that headroom, the longer a cushion we need to ride
  // out a dip — so the target is inversely proportional to it, and a
  // comfortable transcoder keeps a small window (less work thrown away on a
  // seek, less memory held). Clamped at both ends.
  let aheadTarget = AHEAD_MIN_S;
  const speeds: number[] = [];
  let lastSlowLogT = -Infinity;
  const unsubscribeSpeed = subscribeSegmentStat?.((stat) => {
    if (disposed || !Number.isFinite(stat.speedX) || stat.speedX <= 0) return;
    speeds.push(stat.speedX);
    if (speeds.length > SPEED_WINDOW) speeds.shift();
    const avg = speeds.reduce((a, b) => a + b, 0) / speeds.length;
    const headroom = Math.min(1, Math.max(0.25, avg - 1));
    aheadTarget = Math.min(AHEAD_MAX_S, Math.max(AHEAD_MIN_S, AHEAD_MIN_S / headroom));
    notify();
    // Below real time the cushion drains no matter how deep it is. Say so once
    // per 10 s of playback — it is the difference between "this machine cannot
    // do it" and a transient we already absorbed.
    if (avg < 1 && speeds.length >= 4 && video.currentTime - lastSlowLogT > 10) {
      lastSlowLogT = video.currentTime;
      const ahead = videoLane ? bufferedAheadOf(videoLane.sb.buffered, video.currentTime) : 0;
      console.warn(
        `[iris-core] Tier E: transcode below real time — ${avg.toFixed(2)}x ` +
          `over the last ${speeds.length} segments, ${ahead.toFixed(0)}s of cushion left`,
      );
    }
  });

  // Wake the feed gates. `timeupdate` covers steady playback; the rest cover a
  // stalled element, where the buffered ranges still grow as the worker drains
  // its queue and nothing else would tell us. All event-driven — no polling.
  const onProgress = () => notify();
  const WAKE_EVENTS = ["timeupdate", "progress", "waiting", "stalled", "canplay", "playing"];
  for (const e of WAKE_EVENTS) video.addEventListener(e, onProgress);

  const makeInput = (): Input =>
    new Input({
      source: new UrlSource(streamUrl, {
        // Same 5xx-is-transient treatment as Tier B: a redeploy must pause
        // playback, not demote the tier.
        fetchFn: async (url, init) => {
          const res = await fetch(url, init);
          if (res.status >= 500) throw new Error(`iris-stream-transient-5xx ${res.status}`);
          return res;
        },
        getRetryDelay: (attempts) => (attempts >= 12 ? null : Math.min(8, 0.5 * 2 ** attempts)),
      }),
      formats: ALL_FORMATS,
    });

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

  const startPipeline = async (seekStart: number): Promise<void> => {
    setBusy(true);
    // Claim the target NOW, before the demuxer has even been asked where the
    // keyframe is. `effectivePlayhead` is what the chrome's scrubber reads, so
    // without this the position falls back to the pre-seek one for the whole
    // transcode and the seek reads as "it came back". Refined to the keyframe
    // below, once we know it.
    pendingAnchor = seekStart > 0 ? seekStart : null;
    generation += 1;
    const gen = generation;
    await cancelPipelines();
    if (disposed || gen !== generation) return;

    // Flush hevc.js's own backlog, not just ours. The proxy accepts appends
    // eagerly and transcodes them behind our back, so a deep runway means the
    // worker is sitting on tens of seconds of segments for a position the user
    // just left — it would chew through all of them before reaching the new
    // one. `abort()` is what the intercept patches to drop that queue; it also
    // resets the segment parser, which is fine because every restart builds a
    // new Output and therefore emits a fresh init segment.
    for (const lane of [videoLane, audioLane]) {
      if (!lane) continue;
      try {
        if (mediaSource.readyState === "open") lane.sb.abort();
      } catch {
        /* not open, or nothing in flight */
      }
    }
    if (videoLane) {
      videoLane.queue.length = 0;
      videoLane.fedMax = seekStart;
      videoLane.ended = false;
      videoLane.reported = false;
    }
    if (audioLane) {
      audioLane.queue.length = 0;
      audioLane.fedMax = seekStart;
      audioLane.ended = false;
      audioLane.reported = false;
    }

    const liveInput = input;
    if (!liveInput) throw new Error("Tier E: input not initialised");

    const videoTrack = await liveInput.getPrimaryVideoTrack();
    if (!videoTrack) throw new Error("Tier E: no primary video track");
    const videoCodec = await videoTrack.getCodec();
    if (!videoCodec) throw new Error("Tier E: unknown video codec");

    const packetSink = new EncodedPacketSink(videoTrack);
    const startPacket =
      (await packetSink.getKeyPacket(seekStart)) ?? (await packetSink.getFirstKeyPacket());
    if (!startPacket) throw new Error("Tier E: no keyframe found");
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
          `${mediaStart.toFixed(1)}s (${gap.toFixed(1)}s of lead-in would transcode first)`,
      );
    }

    const vOut = new Output({
      format: new Mp4OutputFormat({ fastStart: "fragmented", minimumFragmentDuration: FRAGMENT_S }),
      target: new StreamTarget(sinkFor(videoLane!, gen)),
    });
    relaxMediabunnyGopCheck(vOut);
    const videoSrc = new EncodedVideoPacketSource(videoCodec);
    vOut.addVideoTrack(videoSrc);
    videoOutput = vOut;

    const allAudio = await liveInput.getAudioTracks();
    const audioTrack = allAudio[chosenAudioIdx] ?? null;
    let audioSrc: AudioSampleSource | null = null;
    let audioPassthrough: import("mediabunny").EncodedAudioPacketSource | null = null;
    let aOut: Output | null = null;
    if (audioLane && audioTrack) {
      aOut = new Output({
        format: new Mp4OutputFormat({
          fastStart: "fragmented",
          minimumFragmentDuration: FRAGMENT_S,
        }),
        target: new StreamTarget(sinkFor(audioLane, gen)),
      });
      if (audioNeedsTranscode && encoderChoice) {
        const srcChannels = await audioTrack.getNumberOfChannels();
        audioSrc = new AudioSampleSource({
          codec: encoderChoice.codec,
          // `new Quality(<number>)` is a 0..1 level, not a bitrate — the
          // explicit `{ bitrate }` form is the one that means bits per second.
          quality: new Quality({ bitrate: encoderChoice.codec === "opus" ? 128_000 : 192_000 }),
          ...(encoderChoice.channels !== srcChannels
            ? { transform: { numberOfChannels: encoderChoice.channels } }
            : {}),
        });
        aOut.addAudioTrack(audioSrc);
      } else {
        const { EncodedAudioPacketSource } = await import("mediabunny");
        const codec = await audioTrack.getCodec();
        if (codec) {
          audioPassthrough = new EncodedAudioPacketSource(codec);
          aOut.addAudioTrack(audioPassthrough);
        }
      }
      audioOutput = aOut;
    }

    await vOut.start();
    await aOut?.start();
    if (disposed || gen !== generation) return;

    if (seekStart > 0) initialSeek.done = true;

    const otherFed = (lane: Lane | null): number =>
      lane && !lane.ended ? lane.fedMax : Number.POSITIVE_INFINITY;

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
        if (disposed || gen !== generation) break;
        // Open-GOP leading pictures decode after the random access point but
        // present before it; their references are not in this segment.
        if (packet.timestamp < mediaStart) continue;
        await gate(
          () =>
            disposed ||
            gen !== generation ||
            (packet.timestamp - effectivePlayhead() <= aheadTarget &&
              packet.timestamp - transcodedEnd() <= IN_FLIGHT_CAP_S &&
              packet.timestamp <= otherFed(audioLane) + TRACK_LEAD_CAP_S),
        );
        if (disposed || gen !== generation) break;
        let toAdd = packet;
        if (!first && packet.type !== "key" && packet.timestamp >= nextBoundary) {
          toAdd = new EncodedPacket(packet.data, "key", packet.timestamp, packet.duration);
          boundaryStep = Math.min(FORCED_BOUNDARY_MAX_S, boundaryStep * 2);
          nextBoundary = packet.timestamp + boundaryStep;
        }
        await videoSrc.add(
          toAdd,
          first ? { decoderConfig: decoderConfig ?? undefined } : undefined,
        );
        first = false;
        if (videoLane && packet.timestamp > videoLane.fedMax) videoLane.fedMax = packet.timestamp;
        notify();
      }
      await videoSrc.close();
      if (videoLane) videoLane.ended = true;
      notify();
    })();

    const audioPump = (async () => {
      if (!audioLane || !audioTrack || (!audioSrc && !audioPassthrough)) return;
      if (audioSrc) {
        const sink = new AudioSampleSink(audioTrack);
        for await (const sample of sink.samples(mediaStart, Infinity)) {
          if (disposed || gen !== generation) {
            sample.close();
            break;
          }
          await gate(
            () =>
              disposed ||
              gen !== generation ||
              (sample.timestamp - effectivePlayhead() <= aheadTarget &&
                sample.timestamp <= otherFed(videoLane) + TRACK_LEAD_CAP_S),
          );
          if (disposed || gen !== generation) {
            sample.close();
            break;
          }
          await audioSrc.add(sample);
          if (sample.timestamp > audioLane.fedMax) audioLane.fedMax = sample.timestamp;
          sample.close();
          notify();
        }
        await audioSrc.close();
      } else if (audioPassthrough) {
        const aSink = new EncodedPacketSink(audioTrack);
        const aStart = (await aSink.getKeyPacket(mediaStart)) ?? (await aSink.getFirstKeyPacket());
        if (aStart) {
          let first = true;
          const cfg = await audioTrack.getDecoderConfig();
          for await (const packet of aSink.packets(aStart)) {
            if (disposed || gen !== generation) break;
            await gate(
              () =>
                disposed ||
                gen !== generation ||
                (packet.timestamp - effectivePlayhead() <= aheadTarget &&
                  packet.timestamp <= otherFed(videoLane) + TRACK_LEAD_CAP_S),
            );
            if (disposed || gen !== generation) break;
            await audioPassthrough.add(
              packet,
              first ? { decoderConfig: cfg ?? undefined } : undefined,
            );
            first = false;
            if (packet.timestamp > audioLane.fedMax) audioLane.fedMax = packet.timestamp;
            notify();
          }
        }
        await audioPassthrough.close();
      }
      audioLane.ended = true;
      notify();
    })();

    void Promise.all([videoPump, audioPump]).catch((e: unknown) => {
      if (disposed || gen !== generation) return;
      fail(e instanceof Error ? e : new Error(String(e)));
    });
  };

  input = makeInput();
  try {
    await startPipeline(opts.startPosition);
  } catch (e) {
    setBusy(false);
    const err = e instanceof Error ? e : new Error(String(e));
    fail(err);
    throw err;
  }

  const dispose = async (): Promise<void> => {
    if (disposed) return;
    disposed = true;
    opts.onBusyChange?.(false);
    notify();
    unsubscribeSpeed?.();
    for (const e of WAKE_EVENTS) video.removeEventListener(e, onProgress);
    unbindVideo();
    await cancelPipelines();
    try {
      input?.dispose();
    } catch {
      /* idempotent */
    }
    try {
      if (mediaSource.readyState === "open") mediaSource.endOfStream();
    } catch {
      /* idempotent */
    }
    try {
      video.pause();
      video.removeAttribute("src");
      video.load();
    } catch {
      /* idempotent */
    }
    URL.revokeObjectURL(objectUrl);
    releaseIntercept();
  };

  const isBuffered = (t: number): boolean => {
    if (!videoLane) return false;
    const b = videoLane.sb.buffered;
    for (let i = 0; i < b.length; i += 1) {
      if (b.start(i) - 0.25 <= t && b.end(i) + 0.25 >= t) return true;
    }
    return false;
  };

  const base = videoBackedHandle(video, {
    dispose,
    nativeTrackMap,
    fallbackDuration: manifest.duration_s ?? null,
  });

  const handle: EngineHandle = {
    ...base,
    // While a restart is in flight the element still sits at the old position;
    // report where we are heading instead.
    currentTime: () => effectivePlayhead(),
    seek: (s: number) => {
      const target = Math.max(0, s);
      if (isBuffered(target)) {
        pendingAnchor = null;
        try {
          video.currentTime = target;
        } catch {
          /* swallow */
        }
        return;
      }
      void startPipeline(target).catch((e: unknown) => {
        console.warn("[iris-core] Tier E: seek pipeline failed", e);
      });
    },
  };
  return handle;
};
