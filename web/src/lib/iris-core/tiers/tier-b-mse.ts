/**
 * Tier B — Mediabunny demux + remux to fragmented MP4 → MSE.
 *
 *   /stream → Mediabunny Input
 *     → Mediabunny Output (Mp4OutputFormat, fastStart: 'fragmented')
 *     → StreamTarget (1-second fMP4 fragments)
 *     → MediaSource SourceBuffer.appendBuffer (queued, backpressure-aware)
 *     → `<video>` via `URL.createObjectURL(mediaSource)`
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

import {
  bindVideoCallbacks,
  videoBackedHandle,
  type EngineHandle,
  type EngineMount,
} from "../engine";

export const mountTierB: EngineMount = async (opts) => {
  const { container, manifest, streamUrl, nativeSubs } = opts;
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
  for (const sub of nativeSubs) {
    const track = document.createElement("track");
    track.src = sub.vttUrl;
    track.kind = "subtitles";
    track.label = sub.title ?? sub.lang?.toUpperCase() ?? `Sub ${sub.stream_idx}`;
    track.srclang = sub.lang ?? "und";
    if (sub.default) track.default = true;
    video.appendChild(track);
  }
  container.appendChild(video);

  const initialSeek = { done: false };
  const unbindVideo = bindVideoCallbacks(video, opts, initialSeek);

  const mediaSource = new MediaSource();
  const objectUrl = URL.createObjectURL(mediaSource);
  video.src = objectUrl;

  let disposed = false;
  let conversion: Conversion | null = null;
  let sourceBuffer: SourceBuffer | null = null;
  const appendQueue: Uint8Array[] = [];

  const onErr = () => {
    const err = video.error;
    fail(new Error(err ? `media error ${err.code}: ${err.message}` : "video element error"));
  };
  video.addEventListener("error", onErr);

  const dispose = async (): Promise<void> => {
    if (disposed) return;
    disposed = true;
    unbindVideo();
    video.removeEventListener("error", onErr);
    try {
      await conversion?.cancel();
    } catch {
      /* canceled error is expected */
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

  const handle: EngineHandle = videoBackedHandle(video, { dispose });

  const drainQueue = () => {
    if (disposed || !sourceBuffer || sourceBuffer.updating) return;
    const next = appendQueue.shift();
    if (!next) return;
    try {
      sourceBuffer.appendBuffer(next.slice().buffer);
    } catch (e) {
      fail(e instanceof Error ? e : new Error(String(e)));
    }
  };

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
  const audioCodec = manifest.audio.find((a) => a.default)?.codec_string ?? manifest.audio[0]?.codec_string;
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
  sourceBuffer.addEventListener("updateend", drainQueue);
  sourceBuffer.addEventListener("error", () => fail(new Error("SourceBuffer error")));

  let firstChunkSeen = false;
  const sink = new WritableStream<StreamTargetChunk>({
    write: async (chunk) => {
      if (disposed) return;
      appendQueue.push(chunk.data);
      if (!firstChunkSeen) {
        firstChunkSeen = true;
        opts.onReady?.();
      }
      drainQueue();
      if (appendQueue.length > 4) {
        await new Promise<void>((resolve) => {
          const handler = () => {
            sourceBuffer?.removeEventListener("updateend", handler);
            resolve();
          };
          sourceBuffer?.addEventListener("updateend", handler, { once: true });
        });
      }
    },
    close: () => {
      if (disposed) return;
      try {
        if (mediaSource.readyState === "open") mediaSource.endOfStream();
      } catch {
        /* idempotent */
      }
    },
    abort: (reason) => {
      fail(reason instanceof Error ? reason : new Error(String(reason)));
    },
  });

  const input = new Input({
    source: new UrlSource(streamUrl, {
      requestInit: { credentials: "include" },
    }),
    formats: ALL_FORMATS,
  });
  const output = new Output({
    format: new Mp4OutputFormat({
      fastStart: "fragmented",
      minimumFragmentDuration: 1,
    }),
    target: new StreamTarget(sink),
  });
  try {
    conversion = await Conversion.init({ input, output });
    if (!conversion.isValid) {
      const reasons = conversion.discardedTracks
        .map((t) => `${t.track.codec}: ${t.reason}`)
        .join("; ");
      throw new Error(`Tier B conversion invalid (discarded: ${reasons})`);
    }
    void conversion.execute().catch((e: unknown) => {
      if (e instanceof Error && /canceled/i.test(e.message)) return;
      fail(e instanceof Error ? e : new Error(String(e)));
    });
  } catch (e) {
    await dispose();
    const err = e instanceof Error ? e : new Error(String(e));
    fail(err);
    throw err;
  }

  return handle;
};
