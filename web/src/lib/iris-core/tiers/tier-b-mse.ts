/**
 * Tier B — Mediabunny demux + remux to fragmented MP4 → MSE.
 *
 *   /stream → Mediabunny Input
 *     → Mediabunny Output (Mp4OutputFormat, fastStart: 'fragmented')
 *     → StreamTarget (1-second fMP4 fragments)
 *     → MediaSource SourceBuffer.appendBuffer
 *     → `<video>` via `URL.createObjectURL(mediaSource)`
 *
 * Backpressure: the `WritableStream.write` callback awaits until the
 * SourceBuffer has < BUFFER_AHEAD_TARGET_SECONDS of media buffered
 * ahead of the playhead. This stops Mediabunny from racing past
 * playback and hitting `QuotaExceededError`.
 *
 * Seek out-of-buffer: when the user scrubs to a time the
 * `SourceBuffer.buffered` ranges don't cover, we cancel the current
 * `Conversion` and start a fresh one with `trim: { start: seekTime }`.
 * The `SourceBuffer` is emptied and the `timestampOffset` is set so
 * the new fragments land at the right media time.
 */

import {
  ALL_FORMATS,
  AudioSampleSink,
  AudioSampleSource,
  Conversion,
  EncodedAudioPacketSource,
  EncodedPacketSink,
  EncodedVideoPacketSource,
  Input,
  Mp4OutputFormat,
  Output,
  StreamTarget,
  type StreamTargetChunk,
  UrlSource,
} from "mediabunny";

import { isMobileLike } from "../caps";
import { ensureLibavAudioDecoderRegistered, libavCanDecode } from "../decode/libav-audio-decoder";
import {
  appendNativeTrack,
  bindVideoCallbacks,
  videoBackedHandle,
  type EngineHandle,
  type EngineMount,
} from "../engine";

// ---- Live SourceBuffer window -------------------------------------
//
// The window kept resident in the SourceBuffer is bounded by a SECONDS
// target that is itself sized from the file's average bitrate (byte budget
// ÷ bytes-per-second), then adapted DOWN on a real QuotaExceededError and
// grown back. Rationale:
//
//   - A fixed time window holds `bitrate × seconds` bytes. A 1080p ~8 Mbps
//     remux at 45 s is ~45 MB — fine; a 4K ~40 Mbps remux at the same 45 s
//     pins ~225 MB and OOM-kills the renderer (no JS error → the demote
//     cascade can't save it). So the window is sized from average bitrate:
//     low-bitrate files get a deep window, high-bitrate ones a shallow one.
//   - On VBR, the average undershoots dense stretches, so a real
//     QuotaExceededError pulls the seconds window down to just under what's
//     buffered — the one reliable per-region memory signal.
//   - We do NOT gate on an estimated resident-byte count: the proportional
//     estimate drifted (read ~67 MB for a 28 s buffer) and starved the
//     forward buffer into underruns. The seconds gate is exact now that
//     `bufferedAheadSeconds()` bridges non-coalescing fMP4 ranges.
//
// See `isMobileLike` for the mobile/desktop split.

/** Forward-buffer time ceiling (upper bound; the byte budget can make
 *  the effective window smaller). This is the RELIABLE bound (gated on
 *  `bufferedAheadSeconds`, deadlock-free) — kept modest so the common
 *  low-bitrate case can't pin much memory even if the byte estimate drifts. */
const AHEAD_SECONDS_CEILING = 45;
const AHEAD_SECONDS_CEILING_MOBILE = 25;
/** Firefox desktop ceiling — much lower than Chrome's. Firefox's
 *  per-SourceBuffer eviction threshold is low (it force-evicts, even AHEAD of
 *  the playhead, once resident climbs past ~60 MB — punching holes and wedging
 *  appends in `updating`, which freezes the queue → underrun stall). Chrome's
 *  45 s window can hit ~64 MB on a dense VBR stretch (clampWindow sizes from
 *  the AVERAGE bitrate, which undershoots dense regions), so Firefox gets a
 *  shallow window that keeps resident well under its threshold. */
const AHEAD_SECONDS_CEILING_FIREFOX = 24;
/** Played-out time we keep behind the playhead for instant scrub-back. */
const BEHIND_SECONDS_CEILING = 30;
const BEHIND_SECONDS_CEILING_MOBILE = 15;

/** Memory budget used to SIZE the forward window: `clampWindow` divides it by
 *  the file's average bytes/s to derive a bitrate-aware SECONDS target (the
 *  one back-pressure lever). It is NOT enforced as a runtime byte count — that
 *  per-append estimate proved unreliable (it read ~67 MB for a 28 s buffer,
 *  pinned the producer at the budget, and starved the forward buffer into
 *  underruns). The seconds gate + adaptive shrink-on-real-quota bound memory
 *  instead. The SourceBuffer holds ~this much PLUS Mediabunny's read cache
 *  (`SOURCE_CACHE_BYTES`), so resident ≈ budget + cache. */
const AHEAD_BYTES_BUDGET = 64 * 1024 * 1024;
const AHEAD_BYTES_BUDGET_MOBILE = 20 * 1024 * 1024;

/** Cap for Mediabunny's `UrlSource` read cache (default is 64 MiB, which
 *  stacked on the SourceBuffer budget blew the memory budget). A self-hosted
 *  seedbox is low-latency, so a small cache costs little. */
const SOURCE_CACHE_BYTES = 64 * 1024 * 1024;

/** Firefox-specific desktop budgets. The 96/48 MB desktop window is
 *  tuned for Chrome, whose SourceBuffer quota is generous and whose MSE
 *  eviction is strictly playhead-aware. Firefox's per-SourceBuffer
 *  memory ceiling (`media.mediasource.eviction_threshold.video`) is
 *  lower, and under pressure Firefox will evict data even AHEAD of the
 *  playhead — punching a hole the player can't cross, which freezes
 *  `currentTime` mid-film with no JS error (the "playback dies after a
 *  while, refresh fixes it" report). Keeping the resident window well
 *  under Firefox's threshold stops it from forced-evicting forward
 *  data. Mobile budgets (tighter still) always win when both apply. */
const AHEAD_BYTES_BUDGET_FIREFOX = 48 * 1024 * 1024;

/** Match Firefox-proper + Firefox-derived (LibreWolf, Waterfox, …). */
function isFirefox(): boolean {
  return typeof navigator !== "undefined" && /Firefox\/\d+/.test(navigator.userAgent);
}

/** Floors so a very high-bitrate file can't shrink the forward window to
 *  the point of constant rebuffering. The media is served by-range from
 *  the seedbox (already on disk, low latency), so a small forward window
 *  is acceptable. */
const MIN_AHEAD_SECONDS = 6;
const MIN_BEHIND_SECONDS = 3;

/** Max seconds one track's feed may lead the other. Passthrough video runs
 *  far faster than transcoded audio; uncapped, the muxer holds minutes of the
 *  faster track in RAM waiting to interleave. */
const TRACK_LEAD_CAP = 4;

/** Result of probing `WebCodecs.AudioEncoder`: which target codec
 *  works at what channel count for the given source. Returns null
 *  when neither AAC nor Opus encoding works (caller fails the
 *  Tier B mount and demotes to F).
 *
 *  We probe in priority order:
 *    1. **AAC** — broadest device / receiver compat. Chrome accepts
 *       up to 5.1ch + the `format: 'aac'` field Mediabunny requires
 *       for AAC-in-MP4. Firefox doesn't support AAC-in-MP4 encoding
 *       at all (its WebCodecs AudioEncoder only emits ADTS).
 *    2. **Opus** — Firefox's fallback. 2ch only (browser Opus
 *       encoders are practically stereo-capped). MSE-in-MP4 accepts
 *       `audio/mp4; codecs="opus"` on Chrome + Firefox since ~2020.
 *
 *  Critical: the AAC probe MUST pass `aac: { format: 'aac' }` to
 *  match Mediabunny's own internal config. Firefox returns
 *  `supported: true` on the bare query and then rejects the encoder
 *  once `format` is set — probing without `format` would green-
 *  light Tier B on Firefox and we'd waste a full mount cycle. */
export type AudioEncoderChoice =
  | { codec: "aac"; channels: number; mp4Codec: "mp4a.40.2" }
  | { codec: "opus"; channels: number; mp4Codec: "opus" };

const encoderProbeCache = new Map<string, AudioEncoderChoice | null>();

async function pickAudioEncoder(
  srcChannels: number,
  sampleRate: number,
): Promise<AudioEncoderChoice | null> {
  if (typeof globalThis.AudioEncoder === "undefined") return null;
  const key = `${srcChannels}/${sampleRate}`;
  const cached = encoderProbeCache.get(key);
  if (cached !== undefined) return cached;

  // Pass 1 — AAC at descending channel counts (prefer source layout).
  const aacCandidates = Array.from(
    new Set([srcChannels, 6, 2].filter((n) => n > 0 && n <= srcChannels)),
  );
  for (const n of aacCandidates) {
    try {
      const r = await AudioEncoder.isConfigSupported({
        codec: "mp4a.40.2",
        sampleRate,
        numberOfChannels: n,
        bitrate: 192_000,
        aac: { format: "aac" },
      } as AudioEncoderConfig);
      if (r.supported) {
        const choice: AudioEncoderChoice = {
          codec: "aac",
          channels: n,
          mp4Codec: "mp4a.40.2",
        };
        console.log(
          `[iris-core] Tier B: AudioEncoder → AAC ${n}ch @ ${sampleRate}Hz (source: ${srcChannels}ch)`,
        );
        encoderProbeCache.set(key, choice);
        return choice;
      }
    } catch {
      /* keep walking */
    }
  }

  // Pass 2 — Opus 2ch (Firefox fallback). 128 kbps is around the
  // transparency point for music; speech-heavy content sounds fine
  // well below that, so this is conservative.
  try {
    const r = await AudioEncoder.isConfigSupported({
      codec: "opus",
      sampleRate,
      numberOfChannels: 2,
      bitrate: 128_000,
      opus: { format: "opus" },
    } as AudioEncoderConfig);
    if (r.supported) {
      const choice: AudioEncoderChoice = {
        codec: "opus",
        channels: 2,
        mp4Codec: "opus",
      };
      console.log(
        `[iris-core] Tier B: AudioEncoder → Opus 2ch @ ${sampleRate}Hz (source: ${srcChannels}ch, AAC unavailable)`,
      );
      encoderProbeCache.set(key, choice);
      return choice;
    }
  } catch {
    /* fall through */
  }

  console.log(
    `[iris-core] Tier B: no encodable audio codec @ ${sampleRate}Hz (source: ${srcChannels}ch)`,
  );
  encoderProbeCache.set(key, null);
  return null;
}

/** Mediabunny's MP4 muxer validates that every packet's PTS is ≥ the
 *  max PTS of the previous GOP. That assumption breaks for open-GOP
 *  / deep B-frame video (x265, AV1 with `--b-pyramid normal`, anything
 *  exported by HandBrake with a tight RD), where a new GOP's keyframe
 *  legitimately presents 1 frame before the previous GOP's last
 *  B-frame. The muxer's per-sample PTS/CTS book-keeping handles this
 *  fine, so the only fix needed is to swallow the "previous GOP" error
 *  thrown by the validator. We patch the validator on each Output we
 *  build (the muxer is on `output._muxer`). */
function relaxMediabunnyGopCheck(output: Output): void {
  const m = (
    output as unknown as {
      _muxer?: { validateTimestamp?: (track: unknown, ts: number, isKey: boolean) => void };
    }
  )._muxer;
  if (!m || typeof m.validateTimestamp !== "function") return;
  const original = m.validateTimestamp.bind(m);
  m.validateTimestamp = (track, ts, isKey) => {
    try {
      original(track, ts, isKey);
    } catch (e) {
      if (e instanceof Error && /previous GOP/i.test(e.message)) return;
      throw e;
    }
  };
}

/** Hard cap on undrained append chunks held in RAM. When the drain stalls
 *  (a swallowed QuotaExceededError on a VBR bitrate spike, where the
 *  seconds-derived window holds more bytes than the browser's per-source
 *  ceiling), the producer blocks on this instead of piling decoded GOPs into
 *  memory until the tab OOMs — the failure the Firefox "spam then freeze"
 *  report came from. */
const MAX_QUEUED_CHUNKS = 16;

export const mountTierB: EngineMount = async (opts) => {
  const { container, manifest, streamUrl, nativeSubs, audioTrackIndex } = opts;
  const fail = (err: Error) => opts.onError(err);

  // Derive the live SourceBuffer window up front from the file's average
  // bitrate, capped by both a time ceiling and a byte budget (see the
  // constants above). This is what bounds the renderer's memory on a
  // high-bitrate file — a time-only window would pin hundreds of MB of
  // 4K and OOM-kill the tab on a phone.
  const mobile = isMobileLike();
  const firefox = isFirefox();
  // Mobile budgets win (tightest); Firefox desktop gets its own
  // narrower ceiling; everything else keeps the roomy Chrome window.
  const aheadByteBudget = mobile
    ? AHEAD_BYTES_BUDGET_MOBILE
    : firefox
      ? AHEAD_BYTES_BUDGET_FIREFOX
      : AHEAD_BYTES_BUDGET;
  const bytesPerSecond =
    manifest.size_bytes > 0 && manifest.duration_s && manifest.duration_s > 0
      ? manifest.size_bytes / manifest.duration_s
      : 0;
  const clampWindow = (secondsCeiling: number, minSeconds: number, byteBudget: number): number =>
    bytesPerSecond > 0
      ? Math.min(secondsCeiling, Math.max(minSeconds, byteBudget / bytesPerSecond))
      : secondsCeiling;
  // Forward window in SECONDS — the single back-pressure lever. Sized from the
  // file's AVERAGE bitrate (clampWindow divides the byte budget by bytes/s) so
  // a low-bitrate file gets a deep window and a high-bitrate one a shallow one,
  // capped by the time ceiling. It ADAPTS DOWN on a real QuotaExceededError
  // (the only reliable per-region memory signal on VBR) and grows back.
  //
  // We deliberately do NOT gate on an estimated resident-BYTE count: now that
  // `bufferedAheadSeconds()` correctly bridges non-coalescing fMP4 ranges, the
  // seconds gate bounds the forward buffer accurately, whereas the proportional
  // byte estimate drifted badly (read ~67 MB for a 28 s buffer), pinned the
  // producer at the budget, and starved the forward buffer into underruns.
  let bufferAheadTarget = clampWindow(
    mobile
      ? AHEAD_SECONDS_CEILING_MOBILE
      : firefox
        ? AHEAD_SECONDS_CEILING_FIREFOX
        : AHEAD_SECONDS_CEILING,
    MIN_AHEAD_SECONDS,
    aheadByteBudget,
  );
  const aheadCeiling = bufferAheadTarget;
  // The seek-back (behind) window. Shrinks to the floor on a real quota — it's
  // pure nice-to-have, so we dump it FIRST to free room for the forward buffer
  // playback actually needs. Grows back when quiet. Evicted continuously as
  // the playhead advances.
  let playedKeep = mobile ? BEHIND_SECONDS_CEILING_MOBILE : BEHIND_SECONDS_CEILING;
  const behindCeiling = playedKeep;

  console.log(
    `[iris-core] Tier B buffer window: ahead≤${bufferAheadTarget.toFixed(0)}s ` +
      `behind=${playedKeep.toFixed(0)}s (byteBudget=${(aheadByteBudget / 1e6).toFixed(0)}MB sizes window) ` +
      `(mobile=${mobile}, firefox=${firefox}, ~${((bytesPerSecond * 8) / 1e6).toFixed(1)} Mbps)`,
  );

  if (typeof globalThis.MediaSource === "undefined") {
    const err = new Error("MediaSource Extensions not available");
    fail(err);
    throw err;
  }

  const defaultAudioIdx = Math.max(
    0,
    manifest.audio.findIndex((a) => a.default),
  );
  const chosenAudioIdx = audioTrackIndex ?? defaultAudioIdx;
  const chosenAudio = manifest.audio[chosenAudioIdx];
  const audioNeedsTranscode = chosenAudio != null && !chosenAudio.browser_native;
  if (audioNeedsTranscode && !libavCanDecode(chosenAudio.codec)) {
    const err = new Error(`Tier B: audio codec ${chosenAudio.codec} not transcodable client-side`);
    fail(err);
    throw err;
  }
  if (audioNeedsTranscode) {
    ensureLibavAudioDecoderRegistered();
  }

  // Probe the encoder NOW (before MSE setup) so we know which mp4
  // audio codec to advertise in the SourceBuffer MIME. Uses the
  // manifest's channels + sample_rate — saves a round-trip through
  // Mediabunny's demuxer just to read metadata. Defaults to 48 kHz
  // when the manifest doesn't carry a rate (rare on AC-3 / E-AC-3,
  // which are 48 kHz by spec).
  let encoderChoice: AudioEncoderChoice | null = null;
  if (audioNeedsTranscode && chosenAudio) {
    const srcChannels = chosenAudio.channels ?? 2;
    const srcSampleRate = chosenAudio.sample_rate ?? 48_000;
    encoderChoice = await pickAudioEncoder(srcChannels, srcSampleRate);
    if (!encoderChoice) {
      const err = new Error(
        `Tier B: this browser doesn't support encoding ${srcChannels}ch @ ${srcSampleRate}Hz audio in MP4 (neither AAC nor Opus accepted by AudioEncoder)`,
      );
      fail(err);
      throw err;
    }
  }

  container.innerHTML = "";
  const video = document.createElement("video");
  video.className = "h-full w-full object-contain";
  video.playsInline = true;
  const nativeTrackMap = new Map<number, HTMLTrackElement>();
  for (const sub of nativeSubs) {
    appendNativeTrack(video, sub, nativeTrackMap);
  }
  container.appendChild(video);

  const initialSeek = { done: false };
  const unbindVideo = bindVideoCallbacks(video, opts, initialSeek);

  const mediaSource = new MediaSource();
  const objectUrl = URL.createObjectURL(mediaSource);
  video.src = objectUrl;

  let disposed = false;
  let sourceBuffer: SourceBuffer | null = null;
  // Two parallel pipeline shapes, both writing into the same
  // `appendQueue` / `sourceBuffer`. We keep a reference to whichever
  // is currently feeding so we can cancel it on dispose / seek.
  let conversion: Conversion | null = null;
  let manualOutput: Output | null = null;
  // Mediabunny `Input` — assigned later (after we've validated MSE
  // + opened the MediaSource). Declared up here so `dispose` can
  // close it on any failure path without tripping a TDZ
  // `ReferenceError` on the early throws (codec unsupported, MIME
  // not supported, MediaSource error, …).
  let input: Input | null = null;
  let conversionGeneration = 0;
  // Playback time of the last quota event (gates window re-growth) and of
  // the last quota log line (throttles the console).
  let lastQuotaT = -Infinity;
  let lastQuotaLogT = -Infinity;
  let lastTelemetryT = -Infinity;
  // Diagnostics for the Firefox "appendBuffer wedges in updating=true with no
  // updateend/error" failure (FF bug 1120084). Records which op is in flight; a
  // STALL showing `pendingOp=append updating=true` means the append is wedged
  // (the recovery in `onWaiting` aborts it).
  let pendingOp: "append" | "remove" | null = null;
  // Diagnostics: furthest video timestamp handed to the muxer, and whether
  // the video feed loop has finished. Distinguishes "demux/feed stopped"
  // (fedMax frozen / feedEnded) from "decoder stalled with a full buffer".
  let videoFedMax = 0;
  let videoFeedEnded = false;
  // Furthest AUDIO timestamp handed to the muxer. The video feed is gated so
  // it never races more than TRACK_LEAD_CAP seconds ahead of this (and vice
  // versa): without it, fast passthrough video out-runs the slow transcoded
  // audio by minutes, and the muxer HOLDS all that video in memory waiting to
  // interleave it with audio → jsHeap explodes (300 MB+) while the SourceBuffer
  // itself stays small. The output-side back-pressure can't see that pile-up.
  let audioFedMax = 0;
  // Wakeups for in-flight `waitTrackBalance` calls (resolved when the OTHER
  // track advances, or on dispose). No timers → deadlock-free.
  const trackWaiters = new Set<() => void>();
  const notifyTrackProgress = () => {
    // Each `w()` deletes itself on resolve; deleting the current element
    // during Set iteration is safe, and resolves are async (no waiter is
    // added synchronously during this loop).
    for (const w of trackWaiters) w();
  };
  // Wakeups for feed loops parked in `waitBufferRoom` (the absolute forward
  // back-pressure). Resolved when playback drains the buffer (`timeupdate`)
  // or on dispose. No timers → deadlock-free.
  const bufferRoomWaiters = new Set<() => void>();
  const notifyBufferRoom = () => {
    for (const w of bufferRoomWaiters) w();
  };
  const appendQueue: Uint8Array[] = [];

  // One-shot. Firefox can fire `error` on `<video>` repeatedly
  // when the SourceBuffer is full of undecodable data — without
  // this guard the demote / banner / analytics path runs on every
  // tick. Cosmetic but otherwise floods the console + sends a
  // burst of `playback-error` POSTs.
  let errorFired = false;
  const onErr = () => {
    if (errorFired) return;
    errorFired = true;
    const err = video.error;
    fail(new Error(err ? `media error ${err.code}: ${err.message}` : "video element error"));
  };
  video.addEventListener("error", onErr);

  // ---- buffer helpers ----------------------------------------------

  /** Seconds of media buffered after the current playhead. CRITICAL: walks
   *  forward across ADJACENT ranges, bridging the sub-second gaps between
   *  fMP4 fragments that fail to coalesce into one `buffered` range. Without
   *  the bridge this returned only the first sub-range (e.g. 8 s) while the
   *  SourceBuffer actually held 100 s+ in a dozen touching ranges — so the
   *  back-pressure under-counted wildly, never throttled, and the buffer grew
   *  until it exhausted memory. The bridge makes the back-pressure see the
   *  TRUE forward buffer and bound it. */
  const bufferedAheadSeconds = (): number => {
    if (!sourceBuffer || sourceBuffer.buffered.length === 0) return 0;
    const t = video.currentTime;
    const b = sourceBuffer.buffered;
    let coveredEnd = Number.NEGATIVE_INFINITY;
    for (let i = 0; i < b.length; i += 1) {
      const start = b.start(i);
      const end = b.end(i);
      if (coveredEnd === Number.NEGATIVE_INFINITY) {
        // First range that covers (or sits just after) the playhead.
        if (start <= t + 0.5 && end >= t) coveredEnd = end;
      } else if (start - coveredEnd <= 2) {
        // Adjacent fragment (≤2 s gap — fMP4 fragments often don't coalesce
        // into one range). Bridge it so the back-pressure counts the TRUE
        // forward buffer and throttles promptly (a ≤1 s under-bridge let it
        // overshoot to ~100 s before settling). A genuine hole that wedges
        // playback is larger than this and the playhead can't cross it anyway.
        coveredEnd = end;
      } else {
        break; // genuine gap — the contiguous forward buffer ends here
      }
    }
    return coveredEnd === Number.NEGATIVE_INFINITY ? 0 : Math.max(0, coveredEnd - t);
  };

  const isTimeBuffered = (t: number): boolean => {
    if (!sourceBuffer) return false;
    for (let i = 0; i < sourceBuffer.buffered.length; i += 1) {
      if (sourceBuffer.buffered.start(i) - 0.25 <= t && sourceBuffer.buffered.end(i) + 0.25 >= t) {
        return true;
      }
    }
    return false;
  };

  const waitForUpdateEnd = (): Promise<void> =>
    new Promise<void>((resolve) => {
      if (!sourceBuffer || !sourceBuffer.updating) {
        resolve();
        return;
      }
      sourceBuffer.addEventListener("updateend", () => resolve(), { once: true });
    });

  /** Hold a feed loop until its packet `ts` is within `TRACK_LEAD_CAP` of the
   *  OTHER track's furthest fed timestamp. Keeps fast (passthrough) video from
   *  racing minutes ahead of slow (transcoded) audio and piling up inside the
   *  muxer. Wakes when the other track advances (`notifyTrackProgress`) or on
   *  dispose — never on a timer, so it can't deadlock playback. */
  const waitTrackBalance = (ts: number, otherFedMax: () => number): Promise<void> =>
    new Promise<void>((resolve) => {
      const ready = () => disposed || ts <= otherFedMax() + TRACK_LEAD_CAP;
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

  /** Absolute forward back-pressure — THE memory bound. Holds a feed loop
   *  before it hands the muxer a packet whose timestamp `ts` is more than
   *  `bufferAheadTarget` seconds ahead of the PLAYHEAD.
   *
   *  CRITICAL: it gates on `ts - currentTime` (how far the FEED has run ahead
   *  of playback), NOT on `bufferedAheadSeconds()` (the appended buffer). Two
   *  reasons: (1) Mediabunny's `Output` does NOT propagate StreamTarget write
   *  back-pressure back to `source.add()`, so the source must self-throttle;
   *  (2) when appends lag/stall (Firefox under memory pressure), the appended
   *  metric FREEZES — gating on it let the feed race 150 s+ past the playhead,
   *  piling that media inside the muxer (`fedMax=1770s` while `buffered.end`
   *  was 1614 → 156 s hoarded → heap climbed, then the over-produced append
   *  queue wedged Firefox's SourceBuffer and playback stalled). Bounding
   *  `fed − playhead` caps the muxer backlog regardless of append health.
   *
   *  Wakes when playback advances (`notifyBufferRoom` on `timeupdate`) or on
   *  dispose — never on a timer, so it can't deadlock. */
  const waitBufferRoom = (ts: number): Promise<void> =>
    new Promise<void>((resolve) => {
      const ready = () => disposed || ts - video.currentTime <= bufferAheadTarget;
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

  const evictPlayedRange = (keepSeconds: number): boolean => {
    if (!sourceBuffer || sourceBuffer.updating) return false;
    // Firefox: do NOT run our own `remove()`. Confirmed via telemetry that a
    // `SourceBuffer.remove()` wedges Firefox in `updating=true` forever (no
    // `updateend`/`error` — same swallow as bug 1120084), leaving a stranded
    // zero-width range and freezing every later append → underrun stall. We
    // don't NEED to evict on FF anyway: resident sits ~40 MB while Firefox's
    // native eviction threshold is 150 MiB (`media.mediasource.eviction_
    // threshold.video`), and FF auto-evicts the side FURTHEST from the playhead
    // — always the back buffer, since our feed gate caps the forward buffer at
    // ~24 s. So we leave eviction entirely to Firefox. Chrome keeps our manual
    // eviction (its remove() is reliable and its quota is what we must respect).
    if (firefox) return false;
    const evictBefore = Math.max(0, video.currentTime - keepSeconds);
    if (evictBefore <= 0) return false;
    if (sourceBuffer.buffered.length === 0) return false;
    const firstBufferedStart = sourceBuffer.buffered.start(0);
    if (firstBufferedStart >= evictBefore) return false;
    try {
      sourceBuffer.remove(firstBufferedStart, evictBefore);
      pendingOp = "remove";
      return true;
    } catch {
      return false;
    }
  };

  /** Buffered ranges, for diagnostics — reveals a gap/island (timestamp
   *  issue) vs one contiguous range (pure memory). */
  const bufferedRangesStr = (): string => {
    if (!sourceBuffer || sourceBuffer.buffered.length === 0) return "empty";
    const b = sourceBuffer.buffered;
    const parts: string[] = [];
    for (let i = 0; i < b.length; i += 1) {
      parts.push(`${b.start(i).toFixed(0)}-${b.end(i).toFixed(0)}`);
    }
    return parts.join(" ");
  };

  /** Rough bytes resident in the SourceBuffer: total buffered duration ×
   *  average bitrate. Tracks eviction on BOTH browsers — including Firefox's
   *  NATIVE eviction, which our code doesn't drive — unlike a hand-kept
   *  accumulator (which would only ever grow on FF). Approximate on VBR;
   *  diagnostics only, never a back-pressure input. */
  const residentBytesEstimate = (): number => {
    if (!sourceBuffer || bytesPerSecond <= 0) return 0;
    const b = sourceBuffer.buffered;
    let span = 0;
    for (let i = 0; i < b.length; i += 1) span += b.end(i) - b.start(i);
    return span * bytesPerSecond;
  };

  // ---- queue drain ------------------------------------------------

  const drainQueue = () => {
    if (disposed || !sourceBuffer || sourceBuffer.updating) return;
    const next = appendQueue.shift();
    if (!next) return;
    try {
      sourceBuffer.appendBuffer(next.slice().buffer);
      pendingOp = "append";
    } catch (e) {
      if (e instanceof DOMException && e.name === "QuotaExceededError") {
        // We hit the browser's real per-SourceBuffer byte ceiling — this is the
        // ONE reliable per-region memory signal on VBR. LEARN it: pull the
        // SECONDS window down to just under what's buffered now so the producer
        // parks below the ceiling from here on. Also dump the seek-back buffer
        // (pure nice-to-have) to free the most room for the forward buffer.
        // Both grow back when the high-bitrate stretch passes.
        appendQueue.unshift(next);
        bufferAheadTarget = Math.max(
          MIN_AHEAD_SECONDS,
          Math.min(bufferAheadTarget, bufferedAheadSeconds() * 0.8),
        );
        playedKeep = MIN_BEHIND_SECONDS;
        const freed = evictPlayedRange(playedKeep);
        lastQuotaT = video.currentTime;
        // Throttle the log: at most one line per ~5 s of playback.
        if (video.currentTime - lastQuotaLogT > 5) {
          lastQuotaLogT = video.currentTime;
          console.warn(
            `[iris-core] Tier B: SourceBuffer byte ceiling ` +
              `(ahead=${bufferedAheadSeconds().toFixed(0)}s, ~${(residentBytesEstimate() / 1e6).toFixed(0)}MB, ` +
              `queued=${appendQueue.length}, evicted=${freed}, t=${video.currentTime.toFixed(1)}s) — ` +
              `window→${bufferAheadTarget.toFixed(0)}s ranges=[${bufferedRangesStr()}]`,
          );
        }
        return;
      }
      fail(e instanceof Error ? e : new Error(String(e)));
    }
  };

  // ---- stall recovery (event-driven, no timer) --------------------
  //
  // A `waiting`/`stalled` event means the playhead ran dry. Two
  // mid-playback failure modes this rescues — both surfaced as the
  // "playback dies after a while on Firefox, refresh fixes it" report:
  //
  //   1. A swallowed `QuotaExceededError` (see `drainQueue`) left a
  //      chunk stuck in `appendQueue` with no pending `updateend` to
  //      re-drive the drain — the feed wedges permanently. The
  //      playhead has since advanced, so evicting played-out buffer
  //      now frees space and the re-drain lands the append.
  //   2. Firefox evicted a chunk AHEAD of the playhead under memory
  //      pressure, punching a hole the player can't cross. If a
  //      buffered range resumes just past `currentTime`, nudge across
  //      the gap — the same trick hls.js applies as `nudgeOnVideoHole`.
  //
  // During normal startup buffering this is a harmless no-op: the
  // queue drains as usual and the gap-jump loop finds no near-ahead
  // range to skip to.
  const onWaiting = () => {
    if (disposed || !sourceBuffer) return;
    const t = video.currentTime;
    const wedged = sourceBuffer.updating;
    // Diagnostic: a stall WITH a healthy forward buffer means the decoder
    // choked (bad frame in this rip) — not starvation. A stall AT the
    // buffered end with `videoFeedEnded` means the demuxer stopped feeding.
    // `pendingOp=append updating=true` = the append wedged (FF bug 1120084).
    console.warn(
      `[iris-core] Tier B STALL t=${t.toFixed(1)}s ahead=${bufferedAheadSeconds().toFixed(0)}s ` +
        `fedMax=${videoFedMax.toFixed(0)}s feedEnded=${videoFeedEnded} ` +
        `readyState=${video.readyState} netState=${video.networkState} ` +
        `pendingOp=${pendingOp ?? "none"} updating=${sourceBuffer.updating} queue=${appendQueue.length} ` +
        `err=${video.error ? `${video.error.code}:${video.error.message}` : "none"} ` +
        `ranges=[${bufferedRangesStr()}]`,
    );
    // RECOVERY for a wedged op: if we're starved (`waiting`) yet the SourceBuffer
    // is still `updating` (no `updateend`/`error` ever fired — Firefox bug
    // 1120084 on an open-GOP fragment boundary), the append/remove is hung.
    // `abort()` is the spec-defined reset: it ends the running append/remove
    // algorithm, clears `updating`, and resets the segment parser so the next
    // `appendBuffer` lands. The half-parsed fragment is discarded → a gap we
    // jump below. Without this the pipeline stays frozen until a reload.
    if (wedged) {
      try {
        sourceBuffer.abort();
        pendingOp = null;
        console.warn("[iris-core] Tier B: aborted wedged SourceBuffer op (FF unwedge)");
      } catch {
        /* MediaSource not open — dispose path will clean up */
      }
    }
    evictPlayedRange(playedKeep);
    drainQueue();
    if (isTimeBuffered(t)) return;
    // Jump across a forward gap to the next buffered range. Tolerance is wide
    // enough to clear a whole discarded fragment (Firefox fragments are ~5 s),
    // since after an `abort()` unwedge the buffer resumes a fragment ahead.
    for (let i = 0; i < sourceBuffer.buffered.length; i += 1) {
      const start = sourceBuffer.buffered.start(i);
      const end = sourceBuffer.buffered.end(i);
      // Skip stranded zero-width ranges Firefox leaves behind after remove().
      if (end - start < 0.05) continue;
      if (start > t && start - t < 8) {
        try {
          video.currentTime = start + 0.01;
        } catch {
          /* swallow */
        }
        break;
      }
    }
  };
  video.addEventListener("waiting", onWaiting);
  video.addEventListener("stalled", onWaiting);

  // Re-drive the drain as the playhead advances. A swallowed
  // QuotaExceededError leaves a chunk queued with no pending `updateend`,
  // so the `updateend` pump goes silent — without this, recovery would
  // wait for a full stall (`onWaiting`). Each `timeupdate` (~4 Hz while
  // playing) evicts the freshly played-out buffer and retries the append,
  // so a quota episode self-heals smoothly instead of wedging. No-op when
  // the queue is empty (the healthy case).
  const onTimeUpdate = () => {
    if (disposed || !sourceBuffer) return;
    // Telemetry every ~10 s of playback (gated on currentTime, not a timer):
    // tells us whether resident memory is BOUNDED (buffer too big for the
    // machine) or CLIMBING (a leak), and whether the byte budget engages.
    if (video.currentTime - lastTelemetryT > 10) {
      lastTelemetryT = video.currentTime;
      const heap = (performance as Performance & { memory?: { usedJSHeapSize: number } }).memory
        ?.usedJSHeapSize;
      console.log(
        `[iris-core] Tier B mem: ahead=${bufferedAheadSeconds().toFixed(0)}s ` +
          `fedMax=${videoFedMax.toFixed(0)}s feedEnded=${videoFeedEnded} ` +
          `resident≈${(residentBytesEstimate() / 1e6).toFixed(0)}MB window=${bufferAheadTarget.toFixed(0)}s ` +
          `queue=${appendQueue.length} upd=${sourceBuffer.updating} op=${pendingOp ?? "none"} ` +
          `ranges=[${bufferedRangesStr()}]` +
          (heap ? ` jsHeap=${(heap / 1e6).toFixed(0)}MB` : ""),
      );
    }
    // ALWAYS trim the behind-buffer as the playhead advances — NOT only when
    // there's a queue. If we gate this on `queue > 0`, then once the producer
    // is blocked (byte budget full of un-evicted behind-buffer) the queue
    // drains to empty, no more `updateend` fires, eviction never runs, the
    // behind-buffer keeps growing, the byte budget stays full, the forward
    // buffer starves → underrun/stall. Evicting here frees the budget so the
    // producer can keep the forward buffer alive.
    evictPlayedRange(playedKeep);
    if (appendQueue.length > 0) drainQueue();
    // Playback just advanced → the forward buffer shrank. Release any feed loop
    // parked in `waitBufferRoom` so it tops the buffer back up to the target.
    notifyBufferRoom();
  };
  video.addEventListener("timeupdate", onTimeUpdate);

  /** Seek-restart via Mediabunny's **low-level** API instead of
   *  `Conversion`. `Conversion` always forces a video re-encode when
   *  `trim.start > firstTimestamp` (see `_processVideoTrack`), which
   *  triggers an encoder probe → fails on Chrome for HEVC sources.
   *
   *  Bypassing `Conversion`:
   *   - Video is fed packet-by-packet via `EncodedVideoPacketSource`,
   *     iterating an `EncodedPacketSink` starting at the keyframe
   *     before `seekStart`. Pure passthrough, no encoder needed.
   *   - Audio is fed sample-by-sample via `AudioSampleSource`
   *     (Mediabunny encodes them to AAC with WebCodecs.AudioEncoder)
   *     OR packet-by-packet via `EncodedAudioPacketSource` for
   *     browser-native source codecs. `AudioSampleSink` invokes our
   *     registered libav decoder for E-AC-3/AC-3/FLAC. */
  const restartConversionFromSeek = async (seekStart: number): Promise<void> => {
    try {
      await runManualPipeline(seekStart);
    } catch (e) {
      console.warn(
        "[iris-core] Tier B: manual seek pipeline failed. " +
          "Keeping current playback alive — rewind to a buffered range to resume.",
        e,
      );
    }
  };

  /** Low-level pipeline that handles seek without going through
   *  `Conversion`. See the comment on `restartConversionFromSeek`. */
  const runManualPipeline = async (seekStart: number): Promise<void> => {
    videoFedMax = seekStart;
    audioFedMax = seekStart;
    videoFeedEnded = false;
    const prevConv = conversion;
    const prevOutput = manualOutput;
    const newGen = conversionGeneration + 1;
    // `input` is typed `Input | null` (so `dispose()` can call its
    // `dispose()` safely from early-throw paths before assignment).
    // At this point it must be non-null — the caller is guaranteed
    // to have reached past the assignment in `mountTierB`. Pin a
    // local non-null binding so the rest of the body type-checks
    // cleanly.
    const liveInput = input;
    if (!liveInput) {
      throw new Error("Tier B: runManualPipeline called before input init");
    }

    // **Critical ordering.** Mediabunny's `Output.start()` may emit
    // the fMP4 init segment synchronously through the StreamTarget
    // sink. The sink's `write` callback gates on `generation ===
    // conversionGeneration` to drop stale chunks from cancelled
    // pipelines. If we built the sink with `newGen` while
    // `conversionGeneration` was still the old value, that init
    // segment would be silently dropped — Firefox then receives
    // media fragments with no preceding init and reports
    // `media error 3` (decode error), non-deterministically
    // depending on whether `start()` writes sync or async.
    //
    // To dodge that race we cancel the previous pipelines FIRST,
    // bump `conversionGeneration` to `newGen`, and only THEN build
    // the new Output. The cost is the loss of the previous
    // "init-before-swap" safety property (if `newOutput.start()`
    // throws we have no fallback) — acceptable because init
    // failures here have always meant a hard demote anyway.
    conversionGeneration = newGen;
    try {
      await prevConv?.cancel();
    } catch {
      /* canceled is expected */
    }
    try {
      await prevOutput?.cancel();
    } catch {
      /* idempotent */
    }
    conversion = null;
    manualOutput = null;
    appendQueue.length = 0;

    const sink = buildSink(newGen);
    const newOutput = new Output({
      format: new Mp4OutputFormat({
        fastStart: "fragmented",
        // 1 s fragments on BOTH browsers. We tried 5 s on Firefox (fewer
        // non-coalescing range boundaries) but it made Firefox-macOS's
        // VideoToolbox HARDWARE HEVC decoder throw `media error 3`
        // (`AppleVTDecoder::OnDecodeError`): a 5 s fragment spans several
        // open-GOP boundaries, and the per-GOP composition offsets our muxer
        // patch carries (clamped monotonic DTS) make VT reject a frame. The
        // Firefox STALL we were actually chasing was our own `remove()`
        // wedging `updating=true` (see `evictPlayedRange`), now fixed by NOT
        // evicting on FF — so the larger fragments were never the real lever.
        // 1 s matches Chrome, which decodes the same stream cleanly.
        minimumFragmentDuration: 1,
      }),
      target: new StreamTarget(sink),
    });
    relaxMediabunnyGopCheck(newOutput);

    const videoTrack = await liveInput.getPrimaryVideoTrack();
    if (!videoTrack) throw new Error("manual pipeline: no primary video track");
    const sourceVideoCodec = await videoTrack.getCodec();
    if (!sourceVideoCodec) throw new Error("manual pipeline: unknown video codec");
    const videoSrc = new EncodedVideoPacketSource(sourceVideoCodec);
    newOutput.addVideoTrack(videoSrc);

    const allAudio = await liveInput.getAudioTracks();
    const audioTrack = allAudio[chosenAudioIdx] ?? null;
    type AudioFeed =
      | { kind: "passthrough"; source: EncodedAudioPacketSource }
      | { kind: "transcode"; source: AudioSampleSource };
    let audioFeed: AudioFeed | null = null;
    if (audioTrack) {
      if (audioNeedsTranscode) {
        // Encoder probe ran earlier (outer scope, before MSE setup)
        // so the MIME advertised in `addSourceBuffer` already names
        // the right codec. Here we just hand the choice to
        // Mediabunny.
        if (!encoderChoice) {
          throw new Error("Tier B: internal — audioNeedsTranscode but encoderChoice is null");
        }
        const srcChannels = await audioTrack.getNumberOfChannels();
        const source = new AudioSampleSource({
          codec: encoderChoice.codec,
          bitrate: encoderChoice.codec === "opus" ? 128_000 : 192_000,
          ...(encoderChoice.channels !== srcChannels
            ? { transform: { numberOfChannels: encoderChoice.channels } }
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

    // Pause the video before we touch the SourceBuffer. Firefox is
    // strict about an active playhead crossing through an empty
    // buffered range — it'll fire decode-error events and try to
    // "recover" by re-seeking to whatever's left, which on a fresh
    // remove(0,Inf) is "nothing", so it snaps to 0 and loops.
    // Chrome tolerates this; Firefox doesn't. Remember the pre-
    // seek play state and restore it after the playhead is
    // re-anchored.
    const wasPlaying = !video.paused;
    try {
      video.pause();
    } catch {
      /* idempotent */
    }

    if (sourceBuffer) {
      await waitForUpdateEnd();
      try {
        sourceBuffer.remove(0, Number.POSITIVE_INFINITY);
      } catch {
        /* may already be empty */
      }
      await waitForUpdateEnd();
      // Don't set timestampOffset — Mediabunny emits absolute media
      // timestamps from the source, so SourceBuffer's default 0
      // offset puts fragments at their natural place on the timeline.
      try {
        sourceBuffer.timestampOffset = 0;
      } catch {
        /* swallow */
      }
    }

    // Now start the Output. This emits the fMP4 init segment
    // through the sink; the sink's `generation === conversionGeneration`
    // check now passes (we bumped `conversionGeneration` to `newGen`
    // earlier), so the init lands in `appendQueue` and gets
    // `appendBuffer`'d ahead of any media segments. Without this
    // ordering the init was raced out of the queue and Firefox
    // surfaced `media error 3` non-deterministically.
    await newOutput.start();
    manualOutput = newOutput;

    // Re-anchor the playhead. The seek handler above intentionally
    // does NOT touch `video.currentTime` for out-of-buffer scrubs —
    // we do it here, AFTER the SourceBuffer has been cleared and
    // the init segment has been pushed, so Firefox sees a
    // coherent state from the next `appendBuffer` onward.
    if (seekStart > 0) {
      try {
        if (Math.abs(video.currentTime - seekStart) > 0.05) {
          video.currentTime = seekStart;
        }
      } catch {
        /* swallow */
      }
    }

    // Resume playback (if we paused above) once the playhead is
    // anchored. The video will buffer for a beat before frames
    // arrive — `play()` is a Promise we don't await here, the
    // browser handles the wait → autoplay transition naturally.
    if (wasPlaying) {
      void video.play().catch(() => undefined);
    }

    // Pump video + audio concurrently.
    const videoP = (async () => {
      const packetSink = new EncodedPacketSink(videoTrack);
      const startPacket = await packetSink.getKeyPacket(seekStart);
      if (!startPacket) return;
      const decoderConfig = await videoTrack.getDecoderConfig();
      let firstMeta = true;

      // Feed every packet in decode order. We used to DROP open-GOP "stray"
      // (PTS < keyframe) and "bridge" (PTS ≥ next keyframe) frames to dodge
      // Mediabunny's MP4 muxer `assert(delta >= 0)` — but those are decode
      // references for the open GOP, so dropping them stalled the browser
      // decoder (readyState stuck, no error, on x265 open-GOP rips). The muxer
      // is now patched (iris patch in `mediabunny/.../isobmff-muxer.js`) to
      // clamp the decode timestamp monotonic across GOP boundaries and carry
      // the difference in the signed `trun` composition offset — so we keep
      // every frame and presentation is unchanged.
      for await (const packet of packetSink.packets(startPacket)) {
        if (disposed || newGen !== conversionGeneration) break;
        // Don't race ahead of the audio feed (else the muxer hoards video).
        await waitTrackBalance(packet.timestamp, () => audioFedMax);
        if (disposed || newGen !== conversionGeneration) break;
        // Absolute forward bound: don't out-run playback past the window.
        await waitBufferRoom(packet.timestamp);
        if (disposed || newGen !== conversionGeneration) break;
        const meta = firstMeta ? { decoderConfig: decoderConfig ?? undefined } : undefined;
        await videoSrc.add(packet, meta);
        firstMeta = false;
        if (packet.timestamp > videoFedMax) videoFedMax = packet.timestamp;
        notifyTrackProgress();
      }
      await videoSrc.close();
      // Video done — stop gating audio against a frozen videoFedMax.
      videoFedMax = Number.POSITIVE_INFINITY;
      notifyTrackProgress();
      if (newGen === conversionGeneration && !disposed) {
        videoFeedEnded = true;
        console.warn(
          `[iris-core] Tier B: video feed loop ENDED at fedMax=${videoFedMax.toFixed(1)}s ` +
            `(demuxer reached end-of-stream — if the file isn't fully downloaded or /stream ` +
            `truncates, this is why playback freezes here)`,
        );
      }
    })();

    // No audio track → never gate the video feed on audio.
    if (!(audioTrack && audioFeed)) {
      audioFedMax = Number.POSITIVE_INFINITY;
      notifyTrackProgress();
    }
    const audioP =
      audioTrack && audioFeed
        ? (async () => {
            if (audioFeed.kind === "passthrough") {
              const packetSink = new EncodedPacketSink(audioTrack);
              const startPacket = await packetSink.getKeyPacket(seekStart);
              if (!startPacket) {
                await audioFeed.source.close();
                audioFedMax = Number.POSITIVE_INFINITY;
                notifyTrackProgress();
                return;
              }
              const decoderConfig = await audioTrack.getDecoderConfig();
              let firstMeta = true;
              for await (const packet of packetSink.packets(startPacket)) {
                if (disposed || newGen !== conversionGeneration) break;
                // Don't race ahead of the video feed.
                await waitTrackBalance(packet.timestamp, () => videoFedMax);
                if (disposed || newGen !== conversionGeneration) break;
                await waitBufferRoom(packet.timestamp);
                if (disposed || newGen !== conversionGeneration) break;
                const meta = firstMeta ? { decoderConfig: decoderConfig ?? undefined } : undefined;
                await audioFeed.source.add(packet, meta);
                firstMeta = false;
                if (packet.timestamp > audioFedMax) audioFedMax = packet.timestamp;
                notifyTrackProgress();
              }
              await audioFeed.source.close();
            } else {
              // Transcode: AudioSampleSink uses our registered libav
              // CustomAudioDecoder to decode E-AC-3 → AudioSample (PCM).
              // AudioSampleSource encodes them to AAC via WebCodecs.
              const sampleSink = new AudioSampleSink(audioTrack);
              for await (const sample of sampleSink.samples(seekStart, Infinity)) {
                if (disposed || newGen !== conversionGeneration) {
                  sample.close();
                  break;
                }
                await waitTrackBalance(sample.timestamp, () => videoFedMax);
                if (disposed || newGen !== conversionGeneration) {
                  sample.close();
                  break;
                }
                await waitBufferRoom(sample.timestamp);
                if (disposed || newGen !== conversionGeneration) {
                  sample.close();
                  break;
                }
                await audioFeed.source.add(sample);
                if (sample.timestamp > audioFedMax) audioFedMax = sample.timestamp;
                notifyTrackProgress();
                sample.close();
              }
              await audioFeed.source.close();
            }
            // Audio done — stop gating video against a frozen audioFedMax.
            audioFedMax = Number.POSITIVE_INFINITY;
            notifyTrackProgress();
          })()
        : Promise.resolve();

    void Promise.all([videoP, audioP])
      .then(() => {
        // Don't finalize a pipeline that was canceled (dispose / seek-restart
        // bumped the generation). Calling `finalize()` on a canceled Output
        // throws "Cannot finalize after canceling." — which the catch below
        // used to mistake for a real fault and demote to Tier F on every
        // episode switch.
        if (disposed || newGen !== conversionGeneration) return;
        return newOutput.finalize();
      })
      .catch((e: unknown) => {
        if (disposed || newGen !== conversionGeneration) return;
        // Match canceled / cancelling / "Cannot finalize after canceling".
        if (e instanceof Error && /cancel/i.test(e.message)) return;
        fail(e instanceof Error ? e : new Error(String(e)));
      });
  };

  /** Build a `WritableStream` sink for a fresh Output, scoped to a
   *  particular conversion generation so writes from stale outputs
   *  become no-ops once we bump the generation counter. */
  function buildSink(generation: number): WritableStream<StreamTargetChunk> {
    return new WritableStream<StreamTargetChunk>({
      write: async (chunk) => {
        if (disposed || generation !== conversionGeneration) return;
        appendQueue.push(chunk.data);
        drainQueue();
        while (
          !disposed &&
          generation === conversionGeneration &&
          // Stop feeding when either bound is hit: the (memory-aware, adaptive)
          // seconds window or the in-flight queue cap.
          (bufferedAheadSeconds() > bufferAheadTarget || appendQueue.length > MAX_QUEUED_CHUNKS)
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
              clearTimeout(t);
              resolve();
            };
            sourceBuffer.addEventListener("updateend", done, { once: true });
            const t = setTimeout(done, 500);
          });
        }
      },
      close: () => {
        if (disposed || generation !== conversionGeneration) return;
        try {
          if (mediaSource.readyState === "open") mediaSource.endOfStream();
        } catch {
          /* idempotent */
        }
      },
      abort: (reason) => {
        if (generation !== conversionGeneration) return;
        fail(reason instanceof Error ? reason : new Error(String(reason)));
      },
    });
  }

  // ---- Public handle ---------------------------------------------

  const dispose = async (): Promise<void> => {
    if (disposed) return;
    disposed = true;
    // Release any feed loop parked in `waitTrackBalance` (ready() is now true
    // via `disposed`); the loops then break on their generation check.
    for (const w of trackWaiters) w();
    trackWaiters.clear();
    for (const w of bufferRoomWaiters) w();
    bufferRoomWaiters.clear();
    unbindVideo();
    video.removeEventListener("error", onErr);
    video.removeEventListener("waiting", onWaiting);
    video.removeEventListener("stalled", onWaiting);
    video.removeEventListener("timeupdate", onTimeUpdate);
    try {
      await conversion?.cancel();
    } catch {
      /* canceled is expected */
    }
    try {
      await manualOutput?.cancel();
    } catch {
      /* idempotent */
    }
    // CRITICAL: dispose the Mediabunny `Input` itself. Without this,
    // the UrlSource keeps issuing HTTP range requests in the
    // background, registered CustomAudioDecoders (our libav.js
    // backend) keep emitting AudioSamples that get garbage-collected
    // unclosed (the "AudioSample was garbage collected without first
    // being closed" warning), and — most importantly on Firefox —
    // the live decoders hold AppleVTDecoder / WebCodecs slots from
    // the system pool. Firefox's pool is small enough that a zombie
    // Tier-B Input is enough to starve Tier F's MSE → VT pipeline
    // for video decoder instances, surfacing as
    // `kVTVideoDecoderBadDataErr` on the very first segment append.
    // Chrome's pool is generous enough to mask the leak, which is
    // why this manifested as a "Firefox-only" bug for so long.
    try {
      input?.dispose();
    } catch {
      /* idempotent */
    }
    URL.revokeObjectURL(objectUrl);
    try {
      if (mediaSource.readyState === "open") mediaSource.endOfStream();
    } catch {
      /* idempotent */
    }
    try {
      video.pause();
    } catch {
      /* idempotent */
    }
  };

  const audioTracksFn = () => {
    const defaultIdx = Math.max(
      0,
      manifest.audio.findIndex((x) => x.default),
    );
    const activeIdx = audioTrackIndex ?? defaultIdx;
    return manifest.audio.map((a, i) => ({
      id: String(i),
      label: a.title ?? a.lang?.toUpperCase() ?? `Audio ${i + 1}`,
      lang: a.lang ?? undefined,
      active: i === activeIdx,
    }));
  };

  const baseHandle = videoBackedHandle(video, {
    dispose,
    nativeTrackMap,
    audioTracks: audioTracksFn,
    fallbackDuration: manifest.duration_s ?? null,
  });
  // Override seek so out-of-buffer scrubs trigger a conversion
  // restart. In-buffer scrubs go through the native video element
  // for instant playback.
  const handle: EngineHandle = {
    ...baseHandle,
    seek: (s: number) => {
      const target = Math.max(0, s);
      if (isTimeBuffered(target)) {
        // In-buffer: native video seek is instant, do it now.
        try {
          video.currentTime = target;
        } catch {
          /* swallow */
        }
        return;
      }
      // Out-of-buffer scrub. **Don't** touch `video.currentTime`
      // yet — Firefox handles the gap between "currentTime in
      // unbuffered region" and "first new fragment appended" very
      // poorly: it snap-resets the playhead to the start of the
      // last buffered range (or 0 if we just removed it), fires a
      // spurious decode error, and the resulting `<video error>`
      // event cascades into a Tier B → F demote where hls.js then
      // recover-loops. Chrome is forgiving here; Firefox is not.
      //
      // The manual pipeline (below) clears the SourceBuffer,
      // re-anchors `timestampOffset`, starts pumping, and only
      // THEN sets `video.currentTime = seekStart`. By that point
      // the SourceBuffer's first new fragment is on its way and
      // Firefox sees a coherent state (currentTime + buffered
      // range matching). See `runManualPipeline`.
      void restartConversionFromSeek(target);
    },
  };

  // ---- Initial MediaSource + SourceBuffer setup ------------------

  await new Promise<void>((resolve, reject) => {
    const onOpen = () => {
      mediaSource.removeEventListener("sourceopen", onOpen);
      mediaSource.removeEventListener("error", onMseErr);
      resolve();
    };
    const onMseErr = () => {
      mediaSource.removeEventListener("sourceopen", onOpen);
      mediaSource.removeEventListener("error", onMseErr);
      reject(new Error("MediaSource emitted error before opening"));
    };
    mediaSource.addEventListener("sourceopen", onOpen);
    mediaSource.addEventListener("error", onMseErr);
  });

  const videoCodec = manifest.video[0]?.codec_string;
  // For transcoded audio, advertise whichever codec the
  // AudioEncoder probe selected (AAC on Chrome, Opus on Firefox).
  // For passthrough, use whatever the source already had.
  const audioCodec = audioNeedsTranscode
    ? (encoderChoice?.mp4Codec ?? "mp4a.40.2")
    : chosenAudio?.codec_string;
  const codecs = [videoCodec, audioCodec].filter((c): c is string => !!c).join(",");
  const mime = codecs ? `video/mp4; codecs="${codecs}"` : "video/mp4";
  if (!MediaSource.isTypeSupported(mime)) {
    await dispose();
    const err = new Error(`MIME not supported by MSE: ${mime}`);
    fail(err);
    throw err;
  }

  sourceBuffer = mediaSource.addSourceBuffer(mime);
  sourceBuffer.mode = "segments";

  // Anchor the timeline to the manifest's known duration. Setting
  // this AFTER `addSourceBuffer` matches the order most browsers
  // expect — some Chromium versions throw `InvalidStateError` if
  // duration is set on an empty MediaSource with no source buffers.
  // The `fallbackDuration` on the videoBackedHandle covers the
  // chrome's display in the gap.
  if (manifest.duration_s && manifest.duration_s > 0) {
    try {
      mediaSource.duration = manifest.duration_s;
    } catch (e) {
      console.warn("[iris-core] Tier B: failed to set MediaSource.duration", e);
    }
  }

  sourceBuffer.addEventListener("updateend", () => {
    pendingOp = null;
    evictPlayedRange(playedKeep);
    // Grow the forward window + seek-back window back toward their ceilings
    // once we've been quota-free for a while — restores deep buffering after a
    // transient high-bitrate stretch ends.
    if (video.currentTime - lastQuotaT > 10) {
      if (playedKeep < behindCeiling) {
        playedKeep = Math.min(behindCeiling, playedKeep + 2);
      }
      if (bufferAheadTarget < aheadCeiling) {
        bufferAheadTarget = Math.min(aheadCeiling, bufferAheadTarget + 5);
      }
    }
    drainQueue();
    if (!opts.onReady) return;
    if (sourceBuffer && sourceBuffer.buffered.length > 0) {
      opts.onReady();
      opts.onReady = undefined;
    }
  });
  sourceBuffer.addEventListener("error", () => fail(new Error("SourceBuffer error")));

  // ---- Spin up the first Conversion -----------------------------

  input = new Input({
    source: new UrlSource(streamUrl, {
      // Treat a 5xx as a transient, retryable failure instead of a fatal
      // pipeline error. When the user redeploys, in-flight /stream range
      // requests come back 500/502/503/504. `fetch()` does NOT reject on
      // a bad status, so Mediabunny's default retry (which only fires on
      // a rejected `fetch()`) never kicks in — it throws immediately and
      // the player demotes to Tier F. That's useless (the server is down
      // for F too) and sticky (we stay on the worse tier after recovery).
      // Throwing on 5xx converts it into a rejection that `getRetryDelay`
      // then retries until the backend comes back: playback just pauses
      // (buffer drains, `waiting` fires) and resumes on its own.
      fetchFn: async (input, init) => {
        const res = await fetch(input, init);
        if (res.status >= 500) {
          throw new Error(`iris-stream-transient-5xx ${res.status}`);
        }
        return res;
      },
      // Capped exponential backoff (~0.5,1,2,4,8,8,… s) covering a typical
      // deploy/restart window, then give up so a genuinely broken stream
      // still surfaces (and the WatchPage backstop probe can react). The
      // default never gives up; we bound it to ~12 attempts (~70s).
      getRetryDelay: (attempts) => (attempts >= 12 ? null : Math.min(8, 0.5 * 2 ** attempts)),
      // Cap the source read-ahead cache (default 64 MiB). Stacked on the
      // SourceBuffer budget this was the bulk of the ~160 MB resident that
      // tanked memory; a local seedbox makes a small cache cheap.
      maxCacheSize: SOURCE_CACHE_BYTES,
    }),
    formats: ALL_FORMATS,
  });
  try {
    // Initial mount always goes through the manual pipeline. It:
    //   1. Lets us drop open-GOP straggler B-frames that would
    //      otherwise trip Mediabunny's `assert(delta >= 0)` in the
    //      MP4 muxer's `processTimestamps` (very common on x265
    //      sources, see the videoP loop comment for details).
    //   2. Uses `EncodedPacketSink.getKeyPacket(seekStart)` to jump
    //      via MKV Cues when `startPosition > 0`, avoiding the
    //      hundreds-of-MB linear remux from byte 0 that `Conversion`
    //      did on resume.
    if (opts.startPosition > 0) {
      // Position the playhead at startPosition BEFORE pumping. MSE
      // only fires `canplay` once a buffered range covers the
      // playhead — with currentTime stuck at 0 and buffered =
      // [~startPosition, X], canplay would never come. Suppress
      // `bindVideoCallbacks`' canplay-based initial seek so it
      // doesn't fight us when data finally arrives.
      try {
        video.currentTime = opts.startPosition;
      } catch {
        /* swallow */
      }
      initialSeek.done = true;
    }
    await runManualPipeline(opts.startPosition);
  } catch (e) {
    await dispose();
    const err = e instanceof Error ? e : new Error(String(e));
    fail(err);
    throw err;
  }

  return handle;
};
