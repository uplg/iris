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

import { ensureLibavAudioDecoderRegistered, libavCanDecode } from "../decode/libav-audio-decoder";
import {
  appendNativeTrack,
  bindVideoCallbacks,
  videoBackedHandle,
  type EngineHandle,
  type EngineMount,
} from "../engine";

/** How far ahead of the playhead we let the SourceBuffer fill before
 *  back-pressuring Mediabunny. Larger = more resilient to slow
 *  download + decode bursts, smaller = lower memory footprint. */
const BUFFER_AHEAD_TARGET_SECONDS = 60;

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
  const m = (output as unknown as {
    _muxer?: { validateTimestamp?: (track: unknown, ts: number, isKey: boolean) => void };
  })._muxer;
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

/** Played-out range we keep behind the playhead for instant scrub-back. */
const PLAYED_KEEP_SECONDS = 30;

/** Floor for reactive quota eviction (only used when proactive
 *  back-pressure didn't keep us under the limit — should be rare). */
const QUOTA_EVICT_SECONDS = 5;

export const mountTierB: EngineMount = async (opts) => {
  const { container, manifest, streamUrl, nativeSubs, audioTrackIndex } = opts;
  const fail = (err: Error) => opts.onError(err);

  if (typeof globalThis.MediaSource === "undefined") {
    const err = new Error("MediaSource Extensions not available");
    fail(err);
    throw err;
  }

  const defaultAudioIdx = Math.max(0, manifest.audio.findIndex((a) => a.default));
  const chosenAudioIdx = audioTrackIndex ?? defaultAudioIdx;
  const chosenAudio = manifest.audio[chosenAudioIdx];
  const audioNeedsTranscode = chosenAudio != null && !chosenAudio.browser_native;
  if (audioNeedsTranscode && !libavCanDecode(chosenAudio.codec)) {
    const err = new Error(
      `Tier B: audio codec ${chosenAudio.codec} not transcodable client-side`,
    );
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
  const appendQueue: Uint8Array[] = [];

  const onErr = () => {
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
    try {
      sourceBuffer.remove(firstBufferedStart, evictBefore);
      return true;
    } catch {
      return false;
    }
  };

  // ---- queue drain ------------------------------------------------

  const drainQueue = () => {
    if (disposed || !sourceBuffer || sourceBuffer.updating) return;
    const next = appendQueue.shift();
    if (!next) return;
    try {
      sourceBuffer.appendBuffer(next.slice().buffer);
    } catch (e) {
      if (e instanceof DOMException && e.name === "QuotaExceededError") {
        // The back-pressure loop should normally keep us under the
        // quota, but the SourceBuffer reports a slightly tighter
        // budget than `buffered.end` suggests on some browsers. Try
        // to free space; if we can't (playhead at the start), keep
        // the chunk queued and wait for playback to advance — the
        // back-pressure loop will eventually let us through.
        appendQueue.unshift(next);
        evictPlayedRange(QUOTA_EVICT_SECONDS);
        return;
      }
      fail(e instanceof Error ? e : new Error(String(e)));
    }
  };

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

    // Build the new Output and validate it BEFORE killing the old
    // pipeline. If anything throws here, the running conversion is
    // untouched and playback continues.
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
          throw new Error(
            "Tier B: internal — audioNeedsTranscode but encoderChoice is null",
          );
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

    await newOutput.start();

    // New Output is live. Now safely swap state.
    conversionGeneration = newGen;
    conversion = null;
    manualOutput = newOutput;
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
    appendQueue.length = 0;
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
          if (
            nextKeyPts !== null &&
            pkt.type !== "key" &&
            pkt.timestamp >= nextKeyPts
          ) {
            continue; // bridge frame — drop
          }
          const meta = firstMeta
            ? { decoderConfig: decoderConfig ?? undefined }
            : undefined;
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

    const audioP = audioTrack && audioFeed
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
              const meta = firstMeta
                ? { decoderConfig: decoderConfig ?? undefined }
                : undefined;
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
          bufferedAheadSeconds() > BUFFER_AHEAD_TARGET_SECONDS
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
    const defaultIdx = Math.max(0, manifest.audio.findIndex((x) => x.default));
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
      try {
        video.currentTime = target;
      } catch {
        /* swallow */
      }
      if (isTimeBuffered(target)) return;
      // Out-of-buffer scrub. Best-effort restart; on failure we keep
      // the current conversion alive (see `restartConversionFromSeek`).
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
    ? encoderChoice?.mp4Codec ?? "mp4a.40.2"
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
    evictPlayedRange(PLAYED_KEEP_SECONDS);
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
      requestInit: { credentials: "include" },
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
