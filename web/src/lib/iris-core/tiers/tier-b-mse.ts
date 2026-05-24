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
  type EncodedPacket,
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
// The window kept resident in the SourceBuffer is bounded by BOTH a time
// ceiling and a byte budget, whichever is smaller. Why both:
//
//   - A time-only window (what we had) holds `bitrate × seconds` bytes.
//     A 1080p ~8 Mbps remux at 60 s ahead is ~60 MB — fine. But a 4K
//     ~40 Mbps remux at the same 60 s pins ~300 MB, and stacked on the
//     video decoder + Mediabunny's 64 MiB read cache + the JS heap it
//     gets the renderer OOM-killed on a phone. That OOM raises NO JS
//     error (the demote cascade can't save it), so it MUST be prevented.
//     This is the most likely cause of the mobile "Aïe aïe aïe" crash —
//     a bounded-in-time but unbounded-in-bytes buffer on a high-bitrate
//     file, which is exactly why halving the time window alone wasn't a
//     real fix.
//   - A byte-only window would, on a low-bitrate file, hold an absurd
//     number of seconds (tens of minutes), wasting memory and over-
//     fetching from the seedbox.
//
// So: low-bitrate files get a generous time window; high-bitrate files
// stay capped by the byte budget. See `isMobileLike` for the mobile/
// desktop split.

/** Forward-buffer time ceiling (upper bound; the byte budget can make
 *  the effective window smaller). This is the RELIABLE bound (gated on
 *  `bufferedAheadSeconds`, deadlock-free) — kept modest so the common
 *  low-bitrate case can't pin much memory even if the byte estimate drifts. */
const AHEAD_SECONDS_CEILING = 30;
const AHEAD_SECONDS_CEILING_MOBILE = 20;
/** Played-out time we keep behind the playhead for instant scrub-back. */
const BEHIND_SECONDS_CEILING = 30;
const BEHIND_SECONDS_CEILING_MOBILE = 15;

/** Forward resident-byte budget — the real OOM lever (enforced at runtime
 *  on actual appended bytes, see `residentByteBudget`). Kept deliberately
 *  modest: the SourceBuffer budget is only half the memory story — Mediabunny
 *  reads ~this much SOURCE to fill it PLUS its own read cache (see
 *  `SOURCE_CACHE_BYTES`), so the resident total is roughly budget + cache.
 *  96 MB here meant ~160 MB resident, which OOM-tanked the tab. */
const AHEAD_BYTES_BUDGET = 40 * 1024 * 1024;
const AHEAD_BYTES_BUDGET_MOBILE = 16 * 1024 * 1024;

/** Cap for Mediabunny's `UrlSource` read cache (default is 64 MiB, which
 *  stacked on the SourceBuffer budget blew the memory budget). A self-hosted
 *  seedbox is low-latency, so a small cache costs little. */
const SOURCE_CACHE_BYTES = 24 * 1024 * 1024;

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
const AHEAD_BYTES_BUDGET_FIREFOX = 28 * 1024 * 1024;

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

/** Floor for the adaptive resident-byte budget — below this we'd rebuffer
 *  constantly, so we stop shrinking even if the browser keeps complaining
 *  (at which point the file genuinely can't sustain on this tier). */
const MIN_RESIDENT_BYTES = 16 * 1024 * 1024;

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
  // Forward window as a SECONDS upper bound only (stops a low-bitrate file
  // from buffering tens of minutes). The real memory lever is the BYTE
  // budget enforced at runtime below — a seconds window can't bound bytes on
  // VBR content (60 s at a 30 Mbps stretch is ~225 MB even though the file
  // averages 5 Mbps, which is what exhausted memory and froze playback).
  const bufferAheadTarget = clampWindow(
    mobile ? AHEAD_SECONDS_CEILING_MOBILE : AHEAD_SECONDS_CEILING,
    MIN_AHEAD_SECONDS,
    // Use the full time ceiling here (byte capping is done at runtime).
    Number.POSITIVE_INFINITY,
  );
  // The seek-back (behind) window. Shrinks to the floor on memory pressure —
  // it's pure nice-to-have, so we dump it FIRST to leave the byte budget for
  // the forward buffer that playback actually needs. Grows back when quiet.
  let playedKeep = mobile ? BEHIND_SECONDS_CEILING_MOBILE : BEHIND_SECONDS_CEILING;
  const behindCeiling = playedKeep;

  // ── Runtime byte budget — the actual memory bound. ───────────────────────
  // `residentBytes` tracks the bytes currently in the SourceBuffer (summed on
  // append, reduced proportionally on eviction). The producer stops feeding
  // once it exceeds `residentByteBudget`. The budget starts at the browser's
  // tuned ceiling and ADAPTS DOWN on a real QuotaExceededError (learning the
  // true per-content ceiling), growing back once the high-bitrate stretch
  // passes. This bounds memory regardless of VBR — unlike a seconds window.
  let residentBytes = 0;
  let residentByteBudget = aheadByteBudget;
  console.log(
    `[iris-core] Tier B buffer window: ahead≤${bufferAheadTarget.toFixed(0)}s ` +
      `behind=${playedKeep.toFixed(0)}s byteBudget=${(residentByteBudget / 1e6).toFixed(0)}MB ` +
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

  /** Seconds of media buffered after the current playhead (inside the
   *  range that covers `video.currentTime`). Returns 0 when the
   *  playhead is outside every range. */
  const bufferedAheadSeconds = (): number => {
    if (!sourceBuffer || sourceBuffer.buffered.length === 0) return 0;
    const t = video.currentTime;
    for (let i = 0; i < sourceBuffer.buffered.length; i += 1) {
      const start = sourceBuffer.buffered.start(i);
      const end = sourceBuffer.buffered.end(i);
      if (start <= t + 0.5 && end >= t) return Math.max(0, end - t);
    }
    return 0;
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

  const evictPlayedRange = (keepSeconds: number): boolean => {
    if (!sourceBuffer || sourceBuffer.updating) return false;
    const evictBefore = Math.max(0, video.currentTime - keepSeconds);
    if (evictBefore <= 0) return false;
    if (sourceBuffer.buffered.length === 0) return false;
    const firstBufferedStart = sourceBuffer.buffered.start(0);
    if (firstBufferedStart >= evictBefore) return false;
    const bufferedEnd = sourceBuffer.buffered.end(sourceBuffer.buffered.length - 1);
    try {
      sourceBuffer.remove(firstBufferedStart, evictBefore);
      // Reduce the resident-byte estimate proportionally to the span removed
      // (assumes ~uniform bitrate across the buffer — approximate, but the
      // adaptive budget self-calibrates to whatever scale this produces).
      const span = bufferedEnd - firstBufferedStart;
      if (span > 0) {
        residentBytes = Math.max(
          0,
          residentBytes * (1 - (evictBefore - firstBufferedStart) / span),
        );
      }
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

  // ---- queue drain ------------------------------------------------

  const drainQueue = () => {
    if (disposed || !sourceBuffer || sourceBuffer.updating) return;
    const next = appendQueue.shift();
    if (!next) return;
    try {
      sourceBuffer.appendBuffer(next.slice().buffer);
      residentBytes += next.byteLength;
    } catch (e) {
      if (e instanceof DOMException && e.name === "QuotaExceededError") {
        // We hit the browser's real per-SourceBuffer byte ceiling — the
        // runtime budget over-shot it. LEARN it: pull the budget down to just
        // under what's resident now, so the producer parks below the ceiling
        // from here on (the seconds window can't do this on VBR). Also dump
        // the seek-back buffer (pure nice-to-have) to free the most room for
        // the forward buffer. Both grow back when the stretch passes.
        appendQueue.unshift(next);
        residentByteBudget = Math.max(
          MIN_RESIDENT_BYTES,
          Math.min(residentByteBudget, residentBytes * 0.85),
        );
        playedKeep = MIN_BEHIND_SECONDS;
        const freed = evictPlayedRange(playedKeep);
        lastQuotaT = video.currentTime;
        // Throttle the log: at most one line per ~5 s of playback.
        if (video.currentTime - lastQuotaLogT > 5) {
          lastQuotaLogT = video.currentTime;
          console.warn(
            `[iris-core] Tier B: SourceBuffer byte ceiling ` +
              `(ahead=${bufferedAheadSeconds().toFixed(0)}s, ~${(residentBytes / 1e6).toFixed(0)}MB, ` +
              `queued=${appendQueue.length}, evicted=${freed}, t=${video.currentTime.toFixed(1)}s) — ` +
              `budget→${(residentByteBudget / 1e6).toFixed(0)}MB ranges=[${bufferedRangesStr()}]`,
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
    evictPlayedRange(playedKeep);
    drainQueue();
    const t = video.currentTime;
    if (isTimeBuffered(t)) return;
    for (let i = 0; i < sourceBuffer.buffered.length; i += 1) {
      const start = sourceBuffer.buffered.start(i);
      if (start > t && start - t < 2) {
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
          `resident≈${(residentBytes / 1e6).toFixed(0)}MB budget=${(residentByteBudget / 1e6).toFixed(0)}MB ` +
          `queue=${appendQueue.length} ranges=[${bufferedRangesStr()}]` +
          (heap ? ` jsHeap=${(heap / 1e6).toFixed(0)}MB` : ""),
      );
    }
    if (appendQueue.length > 0) {
      evictPlayedRange(playedKeep);
      drainQueue();
    }
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

      // Open-GOP straggler filter. x265's default settings (open GOP
      // + b-pyramid) produce two kinds of frames that violate
      // Mediabunny's MP4 muxer assumption that "keyframe PTS == min
      // PTS in its GOP":
      //
      //   (1) Post-key strays: packets that decode AFTER a keyframe
      //       but whose PTS is BELOW the keyframe (they present
      //       before the keyframe, referencing the previous GOP).
      //       Detection: PTS < currentKeyPts when received.
      //
      //   (2) Bridge frames: packets that decode at the END of GOP
      //       N but whose PTS is ABOVE GOP N+1's keyframe (they
      //       present after the next keyframe). We only learn the
      //       next keyframe's PTS when it arrives — so we buffer
      //       the current GOP and drop any pkt with PTS ≥ nextKey
      //       on flush.
      //
      // Mediabunny's `processTimestamps` asserts `delta >= 0` when
      // its sorted-PTS DTS assignment dips below `lastTimescaleUnits`
      // from the prev GOP. The two cases above are the only ways
      // this happens for trustworthy demux output. Dropping them
      // loses ~0.7% of frames at GOP boundaries — invisible in
      // playback.

      let currentKeyPts = -Infinity;
      let gopBuffer: EncodedPacket[] = [];

      const flushGop = async (nextKeyPts: number | null): Promise<void> => {
        for (const pkt of gopBuffer) {
          if (nextKeyPts !== null && pkt.type !== "key" && pkt.timestamp >= nextKeyPts) {
            continue; // bridge frame — drop
          }
          const meta = firstMeta ? { decoderConfig: decoderConfig ?? undefined } : undefined;
          await videoSrc.add(pkt, meta);
          firstMeta = false;
        }
        gopBuffer = [];
      };

      for await (const packet of packetSink.packets(startPacket)) {
        if (disposed || newGen !== conversionGeneration) break;
        if (packet.type === "key") {
          await flushGop(packet.timestamp);
          currentKeyPts = packet.timestamp;
          gopBuffer.push(packet);
        } else if (packet.timestamp < currentKeyPts) {
          // post-key stray — drop
          continue;
        } else {
          gopBuffer.push(packet);
        }
      }
      await flushGop(null);
      await videoSrc.close();
    })();

    const audioP =
      audioTrack && audioFeed
        ? (async () => {
            if (audioFeed.kind === "passthrough") {
              const packetSink = new EncodedPacketSink(audioTrack);
              const startPacket = await packetSink.getKeyPacket(seekStart);
              if (!startPacket) {
                await audioFeed.source.close();
                return;
              }
              const decoderConfig = await audioTrack.getDecoderConfig();
              let firstMeta = true;
              for await (const packet of packetSink.packets(startPacket)) {
                if (disposed || newGen !== conversionGeneration) break;
                const meta = firstMeta ? { decoderConfig: decoderConfig ?? undefined } : undefined;
                await audioFeed.source.add(packet, meta);
                firstMeta = false;
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
                await audioFeed.source.add(sample);
                sample.close();
              }
              await audioFeed.source.close();
            }
          })()
        : Promise.resolve();

    void Promise.all([videoP, audioP])
      .then(() => newOutput.finalize())
      .catch((e: unknown) => {
        if (newGen !== conversionGeneration) return;
        if (e instanceof Error && /canceled/i.test(e.message)) return;
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
          // Stop feeding when ANY bound is hit: the seconds upper-bound, the
          // real BYTE budget (the memory lever), or the in-flight queue cap.
          (bufferedAheadSeconds() > bufferAheadTarget ||
            residentBytes > residentByteBudget ||
            appendQueue.length > MAX_QUEUED_CHUNKS)
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
    evictPlayedRange(playedKeep);
    // Grow the byte budget + seek-back window back toward their ceilings once
    // we've been quota-free for a while — restores deep buffering after a
    // transient high-bitrate stretch ends.
    if (video.currentTime - lastQuotaT > 10) {
      if (playedKeep < behindCeiling) {
        playedKeep = Math.min(behindCeiling, playedKeep + 2);
      }
      if (residentByteBudget < aheadByteBudget) {
        residentByteBudget = Math.min(aheadByteBudget, residentByteBudget + 8 * 1024 * 1024);
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
