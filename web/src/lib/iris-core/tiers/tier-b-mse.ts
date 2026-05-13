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
  Conversion,
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
  // Conversion state that we tear down + rebuild on seek-out-of-buffer.
  let conversion: Conversion | null = null;
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

  // ---- Conversion lifecycle --------------------------------------

  /** Build a fresh Mediabunny `Conversion` and execute it.
   *  `mode = 'initial'` is the first mount — no `trim`, browser seeks
   *  to `opts.startPosition` after `canplay`. `mode = 'seek'` is a
   *  scrub outside the buffered range — `trim.start` is applied and
   *  the SourceBuffer's `timestampOffset` aligns new fragments to
   *  the chosen media time.
   *
   *  For `mode = 'seek'`: the new conversion is `init()`'d BEFORE the
   *  old one is canceled, so an `init` failure leaves the running
   *  conversion untouched and the user keeps their buffered playback. */
  const startConversion = async (
    seekStart: number,
    mode: "initial" | "seek",
  ): Promise<void> => {
    const prev = mode === "seek" ? conversion : null;
    const prevGen = conversionGeneration;
    const generation = prevGen + 1;
    const useTrim = mode === "seek" && seekStart > 0;
    if (sourceBuffer && useTrim) {
      try {
        sourceBuffer.timestampOffset = seekStart;
      } catch {
        /* some browsers reject offset during pending append */
      }
    }
    // Back-pressure: wait for SourceBuffer updates (or a 500 ms
    // safety timeout) rather than busy-polling on a short setTimeout
    // — the latter starved Mediabunny's encoder probe and caused
    // `Error when probing encoder support` followed by a hard fail.
    const waitForBufferRoom = (): Promise<void> =>
      new Promise<void>((resolve) => {
        if (
          disposed ||
          generation !== conversionGeneration ||
          bufferedAheadSeconds() <= BUFFER_AHEAD_TARGET_SECONDS
        ) {
          resolve();
          return;
        }
        let settled = false;
        const cleanup = () => {
          if (settled) return;
          settled = true;
          sourceBuffer?.removeEventListener("updateend", onUpdate);
          clearTimeout(timeoutId);
          resolve();
        };
        const onUpdate = () => cleanup();
        sourceBuffer?.addEventListener("updateend", onUpdate, { once: true });
        const timeoutId = setTimeout(cleanup, 500);
      });

    const sink = new WritableStream<StreamTargetChunk>({
      write: async (chunk) => {
        if (disposed || generation !== conversionGeneration) return;
        appendQueue.push(chunk.data);
        drainQueue();
        // Back-pressure loop. We yield only when we're ahead of the
        // playhead by more than the target window.
        while (
          !disposed &&
          generation === conversionGeneration &&
          bufferedAheadSeconds() > BUFFER_AHEAD_TARGET_SECONDS
        ) {
          await waitForBufferRoom();
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

    const audioFilter = (_track: unknown, n: number) => {
      if (n !== chosenAudioIdx + 1 && chosenAudioIdx >= 0) return { discard: true };
      if (audioNeedsTranscode) return { codec: "aac" as const, bitrate: 192_000 };
      return {};
    };

    const output = new Output({
      format: new Mp4OutputFormat({
        fastStart: "fragmented",
        minimumFragmentDuration: 1,
      }),
      target: new StreamTarget(sink),
    });

    // No `video` option here — Mediabunny's default is passthrough
    // when the target container supports the source codec. Setting
    // `video: { codec: 'hevc' }` actually means "transcode to HEVC",
    // which forces Mediabunny down a re-encode path that fails on
    // Chrome (no HEVC encoder) and produces a black screen.
    const newConversion = await Conversion.init({
      input,
      output,
      audio: audioFilter,
      ...(useTrim ? { trim: { start: seekStart } } : {}),
    });
    if (disposed) {
      try {
        await newConversion.cancel();
      } catch {
        /* idempotent */
      }
      return;
    }
    if (!newConversion.isValid) {
      const reasons = newConversion.discardedTracks
        .map((t) => `${t.track.codec}: ${t.reason}`)
        .join("; ");
      try {
        await newConversion.cancel();
      } catch {
        /* idempotent */
      }
      throw new Error(`Tier B conversion invalid (discarded: ${reasons})`);
    }

    // Init succeeded. Now safe to swap state.
    if (prev) {
      try {
        await prev.cancel();
      } catch {
        /* canceled is expected */
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
        if (useTrim) {
          try {
            sourceBuffer.timestampOffset = seekStart;
          } catch {
            /* swallow */
          }
        }
      }
    }
    conversion = newConversion;
    conversionGeneration = generation;
    void newConversion.execute().catch((e: unknown) => {
      if (generation !== conversionGeneration) return;
      if (e instanceof Error && /canceled/i.test(e.message)) return;
      fail(e instanceof Error ? e : new Error(String(e)));
    });
  };


  /** Best-effort seek-restart. Tries to spin up a fresh Mediabunny
   *  `Conversion` with `trim.start = seekStart` and swap it in for
   *  the currently-running one. If init fails (most commonly:
   *  Mediabunny's encoder-probe path with HEVC video + audio
   *  transcode — Chrome has no HEVC encoder, the probe throws), we
   *  bail without touching the running conversion so the user can
   *  keep playing the buffered range and rewind. No demote to F. */
  const restartConversionFromSeek = async (seekStart: number): Promise<void> => {
    try {
      await startConversion(seekStart, "seek");
    } catch (e) {
      console.warn(
        "[iris-core] Tier B: seek-restart failed (Mediabunny init threw). " +
          "Keeping current playback alive. Rewind to a buffered range to resume.",
        e,
      );
      // Try to revive the previous conversion's generation so its
      // in-flight writes (if any) are still accepted. The buffered
      // ranges are still there — the user can scrub back.
      // (We can't actually "uncancel" a Conversion, but if the new
      // init threw early enough, the old one was never canceled.)
    }
  };

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
  const audioCodec = audioNeedsTranscode ? "mp4a.40.2" : chosenAudio?.codec_string;
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

  const input = new Input({
    source: new UrlSource(streamUrl, {
      requestInit: { credentials: "include" },
    }),
    formats: ALL_FORMATS,
  });
  try {
    // Initial mount: no `trim`. The browser seeks to
    // `opts.startPosition` after `canplay` (handled by
    // `bindVideoCallbacks`). Trim is only used for seek-restart
    // scrubs — that path had buggy interaction with Mediabunny's
    // encoder probe on the first run.
    await startConversion(0, "initial");
  } catch (e) {
    await dispose();
    const err = e instanceof Error ? e : new Error(String(e));
    fail(err);
    throw err;
  }

  return handle;
};
