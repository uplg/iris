/**
 * Tier C live — WebCodecs decode → hardware re-encode → single MSE
 * `<video>` element. One engine for every browser with WebCodecs.
 *
 * The lesson of this engine's history: only the browser can align
 * audio and video to the SPEAKER — its media element compensates the
 * audio output latency internally (YouTube is lip-perfect on the same
 * machine where every canvas-chasing-a-clock architecture stayed
 * hundreds of ms off, because no JS-visible clock reflects what the
 * ear actually receives). So the presentation MUST live inside one
 * media element. But the broadcast stream can't be fed to MSE as-is
 * (open-GOP TNT H.264 without IDR: Chromium's strict decoder kills
 * the pipeline on every mid-stream join; Firefox's VT conceals
 * erratically). The answer:
 *
 *   master.m3u8 → mediabunny Input (HLS live, gapless timeline)
 *     video → WebCodecs VideoDecoder            (JOIN CONCEALMENT OURS:
 *              reset ladder rejoins forward)     the broadcast quirks
 *     → VideoDecoder output frames               never reach MSE
 *     → WebCodecs VideoEncoder (H.264, hardware, closed GOP, real
 *       IDRs every 2 s) — a CANONICAL stream both browsers accept
 *     audio → AudioSampleSink (libav E-AC-3 → PCM) → AudioEncoder
 *       (AAC Chromium / Opus Firefox), outputs stamped by pure
 *       bookkeeping (fixed frame sizes) — encoder stamps are ignored
 *     → mediabunny Output (one fMP4, both tracks)
 *     → ONE `<video>` SourceBuffer → the browser presents, natively
 *       A/V-synced, latency-compensated, like any other site.
 *
 * Both tracks are re-stamped to `ts − anchor`: the element timeline
 * starts at 0, and video output chunks take their timestamps from a
 * FIFO of the frames WE fed (1:1 in realtime mode, no B-frames) — no
 * encoder stamp is ever trusted.
 *
 * No server re-encode anywhere: the tuner keeps shipping `-c copy`
 * broadcast bytes; the re-encode is client-side, hardware, ~few % CPU.
 *
 * Timer note: pacing loops poll with short setTimeout waits — the same
 * pattern as VOD tier C (`video-pipeline.ts`, `tier-c-webcodecs.ts`).
 */

import {
  ALL_FORMATS,
  AudioSampleSink,
  EncodedAudioPacketSource,
  EncodedPacket,
  EncodedPacketSink,
  EncodedVideoPacketSource,
  Input,
  Mp4OutputFormat,
  Output,
  StreamTarget,
  type StreamTargetChunk,
  UrlSource,
} from "mediabunny";

import { refreshSessionForFetch } from "../../api";
import { ensureLibavAudioDecoderRegistered, libavCanDecode } from "../decode/libav-audio-decoder";
import { configWithFreshDescription } from "../decode/webcodecs-probe";
import {
  bindVideoCallbacks,
  videoBackedHandle,
  type EngineHandle,
  type EngineMount,
} from "../engine";
import { pickAudioEncoder, relaxMediabunnyGopCheck } from "./tier-b-mse";

/** How far behind the playlist's end we aim the first keyframe. */
const LIVE_EDGE_BACKOFF_S = 12;
/** Don't anchor until the playlist holds at least this much media
 *  (fresh backend sessions start with a 1-2 segment window). Kept low:
 *  every second here is zap latency, and the engine now survives a thin
 *  initial cushion (the element just rebuffers briefly). */
const MIN_START_WINDOW_S = 3;
/** Feed bound past the element playhead — the element's buffer target.
 *  Must comfortably exceed the muxer's video-fragment burst size (one
 *  re-encoded GOP): fragments only cut on IDRs, so media arrives in
 *  GOP-sized bursts while playback drains continuously. With a margin
 *  ≈ the burst size the buffer sawtooths to zero and the picture
 *  micro-freezes on every slightly-late burst (the deterministic
 *  "tick"). 6 s of margin over 1 s GOPs ends that. */
const FEED_AHEAD_S = 6;
/** Pre-start runway. Until the first fragment lands the playhead sits
 *  at 0, and the join concealment can push the first video keyframe a
 *  couple seconds in — the muxer then needs AUDIO past firstKey+GOP to
 *  cut its first fragment. A tight pre-start pacer deadlocks the whole
 *  triangle (audio waits playback, muxer waits audio, playback waits
 *  muxer); 8 s clears any join comfortably. */
const PRESTART_AHEAD_S = 8;
/** Max seconds one track's feed may lead the other (muxer interleave
 *  holds the difference in RAM). */
const TRACK_LEAD_CAP = 4;
/** Played-out media kept in the SourceBuffer. */
const KEEP_BEHIND_S = 30;
/** Re-encoded GOP length — a real IDR every second. Keeps the muxer's
 *  fragment bursts small (see FEED_AHEAD_S) and the first fragment of a
 *  join fast. */
const ENC_GOP_FRAMES = 50; // 1 s at 50 fps
/** Video decoder recreations tolerated per rolling minute (join
 *  concealment burns 1-4 while the DPB fills). */
const RESET_BUDGET = 12;
const RESET_WINDOW_MS = 60_000;
/** Pacing poll interval — tier C's established backpressure pattern. */
const PACE_MS = 100;

/** Codecs MSE plays inside fMP4 without help — audio passthrough. */
const MSE_NATIVE_AUDIO = new Set(["aac", "opus", "mp3"]);

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

export const mountTierCLive: EngineMount = async (opts) => {
  const { container, streamUrl } = opts;
  const fail = (err: Error) => opts.onError(err);

  if (
    typeof globalThis.VideoDecoder === "undefined" ||
    typeof globalThis.VideoEncoder === "undefined" ||
    typeof globalThis.MediaSource === "undefined"
  ) {
    const err = new Error("live: WebCodecs/MSE unavailable");
    fail(err);
    throw err;
  }

  container.innerHTML = "";
  const video = document.createElement("video");
  video.className = "h-full w-full object-contain";
  video.playsInline = true;
  container.appendChild(video);
  const initialSeek = { done: true }; // live: no VOD resume seek
  const unbindVideo = bindVideoCallbacks(video, opts, initialSeek);

  let disposed = false;
  let input: Input | null = null;
  let decoder: VideoDecoder | null = null;
  let encoder: VideoEncoder | null = null;
  let output: Output | null = null;
  let mediaSrc: MediaSource | null = null;
  let sourceBuffer: SourceBuffer | null = null;
  let objectUrl: string | null = null;
  const appendQueue: Uint8Array[] = [];
  let anchor = 0;
  /** Element-relative feed positions, for pacing + interleave caps. */
  let videoFedRel = 0;
  let audioFedRel = 0;

  /** Element playhead in the rel timeline — the pacing reference. The
   *  browser owns actual A/V presentation; this only throttles work. */
  const playheadRel = (): number => video.currentTime;

  const drainQueue = () => {
    if (disposed || !sourceBuffer || sourceBuffer.updating) return;
    const next = appendQueue.shift();
    if (!next) return;
    try {
      sourceBuffer.appendBuffer(next.slice().buffer);
    } catch (e) {
      if (e instanceof DOMException && e.name === "QuotaExceededError") {
        appendQueue.unshift(next);
        evictPlayed();
        return;
      }
      fail(e instanceof Error ? e : new Error(String(e)));
    }
  };

  const evictPlayed = () => {
    if (!sourceBuffer || sourceBuffer.updating) return;
    const evictBefore = video.currentTime - KEEP_BEHIND_S;
    if (evictBefore <= 0 || sourceBuffer.buffered.length === 0) return;
    const first = sourceBuffer.buffered.start(0);
    if (first >= evictBefore) return;
    try {
      sourceBuffer.remove(first, evictBefore);
    } catch {
      /* retried next tick */
    }
  };

  /** Jump small forward holes (a decoder-reset gap leaves a video hole
   *  in the muxed stream; the element parks at its edge). */
  const jumpForwardGap = () => {
    if (!sourceBuffer) return;
    const t = video.currentTime;
    for (let i = 0; i < sourceBuffer.buffered.length; i += 1) {
      const start = sourceBuffer.buffered.start(i);
      const end = sourceBuffer.buffered.end(i);
      if (end - start < 0.05) continue;
      if (start > t && start - t < 8) {
        console.warn(`[iris-core] live-c: gap jump ${t.toFixed(2)} → ${start.toFixed(2)}`);
        try {
          video.currentTime = start + 0.01;
        } catch {
          /* swallow */
        }
        break;
      }
    }
  };
  const onWaiting = () => {
    if (disposed || !sourceBuffer) return;
    if (sourceBuffer.updating) {
      try {
        sourceBuffer.abort();
      } catch {
        /* MediaSource not open */
      }
    }
    drainQueue();
    jumpForwardGap();
  };
  video.addEventListener("waiting", onWaiting);
  video.addEventListener("stalled", onWaiting);
  const onTimeUpdate = () => {
    if (disposed) return;
    evictPlayed();
    if (appendQueue.length > 0) drainQueue();
  };
  video.addEventListener("timeupdate", onTimeUpdate);

  const dispose = async (): Promise<void> => {
    if (disposed) return;
    disposed = true;
    unbindVideo();
    video.removeEventListener("waiting", onWaiting);
    video.removeEventListener("stalled", onWaiting);
    video.removeEventListener("timeupdate", onTimeUpdate);
    try {
      decoder?.close();
    } catch {
      /* idempotent */
    }
    try {
      encoder?.close();
    } catch {
      /* idempotent */
    }
    try {
      await output?.cancel();
    } catch {
      /* idempotent */
    }
    try {
      video.pause();
    } catch {
      /* idempotent */
    }
    if (objectUrl) URL.revokeObjectURL(objectUrl);
    video.removeAttribute("src");
    try {
      if (mediaSrc && mediaSrc.readyState === "open") mediaSrc.endOfStream();
    } catch {
      /* idempotent */
    }
    try {
      input?.dispose();
    } catch {
      /* idempotent */
    }
  };

  try {
    input = new Input({
      source: new UrlSource(streamUrl, {
        fetchFn: async (fetchInput, init) => {
          let res = await fetch(fetchInput, init);
          // 401/403: the access token expired mid-stream. These raw
          // fetches don't ride the api client's 401-retry, so refresh the
          // session OURSELVES (single-flight, shared with the app) and
          // replay once.
          if (res.status === 401 || res.status === 403) {
            if (await refreshSessionForFetch()) {
              res = await fetch(fetchInput, init);
            }
          }
          // 5xx (and an auth failure that survived the refresh): transient
          // — reject so mediabunny's retry ladder takes over.
          if (res.status >= 500 || res.status === 401 || res.status === 403) {
            throw new Error(`iris-live-transient-${res.status}`);
          }
          return res;
        },
        getRetryDelay: (attempts) => (attempts >= 12 ? null : Math.min(8, 0.5 * 2 ** attempts)),
        maxCacheSize: 32 * 1024 * 1024,
      }),
      formats: ALL_FORMATS,
      formatOptions: { hls: { offsetTimestampsByDateTime: false } },
    });

    const videoTrack = await input.getPrimaryVideoTrack();
    if (!videoTrack) throw new Error("live: no video track");
    const videoConfig = await videoTrack.getDecoderConfig();
    if (!videoConfig?.codec) throw new Error("live: video codec unknown");
    const decSupport = await VideoDecoder.isConfigSupported(
      configWithFreshDescription(videoConfig),
    );
    if (!decSupport.supported) {
      throw new Error(`live: WebCodecs cannot decode ${videoConfig.codec}`);
    }
    // Canonical re-encode target: fixed 16:9 1080p. The DECODED frames may
    // not match — Firefox emits interlaced content as half-height FIELD
    // frames (1920×540), and feeding those to a 1080p-configured encoder
    // letterboxes them (the giant top/bottom bars) instead of stretching.
    // Mismatched frames go through an OffscreenCanvas stretch-blit (bob
    // deinterlacing, the standard TV treatment) before encoding.
    const width = 1920;
    const height = 1080;
    const encConfig: VideoEncoderConfig = {
      codec: "avc1.640028",
      width,
      height,
      bitrate: 8_000_000,
      framerate: 50,
      latencyMode: "realtime",
      avc: { format: "avc" },
    };
    const encSupport = await VideoEncoder.isConfigSupported(encConfig);
    if (!encSupport.supported) {
      throw new Error("live: no H.264 encoder available for the canonical re-encode");
    }

    const audioTrack = (await input.getAudioTracks())[0] ?? null;
    const audioCodec = audioTrack ? await audioTrack.getCodec() : null;
    type AudioPlan =
      | { kind: "passthrough"; mp4Codec: string }
      | { kind: "transcode"; mp4Codec: string; targetCodec: "aac" | "opus"; channels: number };
    let audioPlan: AudioPlan | null = null;
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
        console.warn(`[iris-core] live-c: audio codec ${audioCodec} undecodable — video only`);
      }
    }

    // Anchor near the live edge from playlist metadata (never the client
    // clock); wait out fresh sessions' thin window. skipLiveWait is
    // load-bearing (without it: resolves only when the stream ENDS).
    const videoPacketSink = new EncodedPacketSink(videoTrack);
    let windowEnd = (await input.getDurationFromMetadata(undefined, { skipLiveWait: true })) ?? 0;
    const windowWait0 = performance.now();
    while (
      !disposed &&
      windowEnd < MIN_START_WINDOW_S &&
      performance.now() - windowWait0 < 30_000
    ) {
      await sleep(500);
      windowEnd = (await input.getDurationFromMetadata(undefined, { skipLiveWait: true })) ?? 0;
    }
    const edgeTarget = Math.max(0, windowEnd - LIVE_EDGE_BACKOFF_S);
    let startPacket = await videoPacketSink.getKeyPacket(edgeTarget);
    if (!startPacket) startPacket = await videoPacketSink.getFirstKeyPacket();
    if (!startPacket) throw new Error("live: no video keyframe in window");
    anchor = startPacket.timestamp;
    console.log(
      `[iris-core] live-c mount: dec=${videoConfig.codec} → enc=${encConfig.codec} ` +
        `(${encSupport.config?.hardwareAcceleration ?? "hw?"}) audio=${audioCodec ?? "none"}` +
        `${audioPlan ? ` (${audioPlan.kind} → ${audioPlan.mp4Codec})` : ""} ` +
        `window=${windowEnd.toFixed(1)}s cushion=${(windowEnd - anchor).toFixed(1)}s ` +
        `presentation=single-element`,
    );

    // ---- MSE on the ONE element ----------------------------------------
    const ms = new MediaSource();
    mediaSrc = ms;
    objectUrl = URL.createObjectURL(ms);
    video.src = objectUrl;
    await new Promise<void>((resolve, reject) => {
      const onOpen = () => {
        ms.removeEventListener("sourceopen", onOpen);
        ms.removeEventListener("error", onMseErr);
        resolve();
      };
      const onMseErr = () => {
        ms.removeEventListener("sourceopen", onOpen);
        ms.removeEventListener("error", onMseErr);
        reject(new Error("live: MediaSource errored before opening"));
      };
      ms.addEventListener("sourceopen", onOpen);
      ms.addEventListener("error", onMseErr);
    });
    if (disposed) throw new Error("live: disposed during MediaSource open");

    const mime = `video/mp4; codecs="${[encConfig.codec, audioPlan?.mp4Codec]
      .filter(Boolean)
      .join(", ")}"`;
    if (!MediaSource.isTypeSupported(mime)) {
      throw new Error(`live: MIME unsupported: ${mime}`);
    }
    const sb = ms.addSourceBuffer(mime);
    sb.mode = "segments";
    sourceBuffer = sb;
    let playbackStarted = false;
    let playheadAnchored = false;
    let firstBufferedWall = 0;
    // Deferred start: show the first frame the moment it exists (anchor
    // the paused playhead = instant poster), but only START playing once
    // a real buffer cushion has built. A cold backend session joins with
    // a 2-4 s playlist window — playing immediately rides the live edge
    // with zero margin and stutters for a minute (the buffer can only
    // grow at the price of stalls). Waiting ~4 s ONCE beats that; warm
    // sessions cross the threshold instantly.
    const START_BUFFER_S = 4;
    const START_MAX_WAIT_MS = 8_000;
    sb.addEventListener("updateend", () => {
      if (disposed) return;
      drainQueue();
      if (playbackStarted || sb.buffered.length === 0) return;
      if (!playheadAnchored) {
        playheadAnchored = true;
        firstBufferedWall = performance.now();
        opts.onReady?.();
        opts.onReady = undefined;
        try {
          video.currentTime = sb.buffered.start(0) + 0.05;
        } catch {
          /* swallow */
        }
      }
      const ahead = sb.buffered.end(sb.buffered.length - 1) - video.currentTime;
      if (ahead >= START_BUFFER_S || performance.now() - firstBufferedWall > START_MAX_WAIT_MS) {
        playbackStarted = true;
        console.log(`[iris-core] live-c: starting playback (buffer=${ahead.toFixed(1)}s)`);
        void video.play().catch(() => {
          console.warn("[iris-core] live-c: autoplay blocked — press play");
        });
      }
    });
    sb.addEventListener("error", () => {
      if (!disposed) fail(new Error("live: SourceBuffer error"));
    });

    // ---- one muxer, both tracks -----------------------------------------
    const muxOutput = new Output({
      format: new Mp4OutputFormat({ fastStart: "fragmented", minimumFragmentDuration: 0.5 }),
      target: new StreamTarget(
        new WritableStream<StreamTargetChunk>({
          write: (chunk) => {
            if (disposed) return;
            appendQueue.push(chunk.data);
            drainQueue();
          },
          close: () => {
            if (disposed) return;
            try {
              if (ms.readyState === "open") ms.endOfStream();
            } catch {
              /* idempotent */
            }
          },
          abort: (reason) => {
            if (disposed) return;
            fail(reason instanceof Error ? reason : new Error(String(reason)));
          },
        }),
      ),
    });
    relaxMediabunnyGopCheck(muxOutput);
    output = muxOutput;

    const videoSrc = new EncodedVideoPacketSource("avc");
    muxOutput.addVideoTrack(videoSrc);

    type AudioFeed =
      | { kind: "passthrough"; source: EncodedAudioPacketSource }
      | { kind: "manual"; source: EncodedAudioPacketSource };
    let audioFeed: AudioFeed | null = null;
    if (audioTrack && audioPlan) {
      const source = new EncodedAudioPacketSource(
        audioPlan.kind === "passthrough"
          ? ((await audioTrack.getCodec()) ?? "aac")
          : audioPlan.targetCodec,
      );
      muxOutput.addAudioTrack(source);
      audioFeed = { kind: audioPlan.kind === "passthrough" ? "passthrough" : "manual", source };
    }
    await muxOutput.start();

    // ---- audio feed (bookkeeping-stamped, encoder stamps ignored) --------
    if (audioTrack && audioPlan && audioFeed) {
      const plan = audioPlan;
      const feed = audioFeed;
      const feedAudio = async () => {
        if (feed.kind === "passthrough") {
          const packetSink = new EncodedPacketSink(audioTrack);
          let start = await packetSink.getKeyPacket(anchor);
          if (!start) start = await packetSink.getFirstKeyPacket();
          if (!start) return;
          const decoderConfig = await audioTrack.getDecoderConfig();
          let firstMeta = true;
          for await (const packet of packetSink.packets(start)) {
            if (disposed) break;
            const rel = Math.max(0, packet.timestamp - anchor);
            while (
              !disposed &&
              (rel - playheadRel() > (video.readyState > 0 ? FEED_AHEAD_S : PRESTART_AHEAD_S) ||
                rel - videoFedRel > TRACK_LEAD_CAP)
            ) {
              await sleep(PACE_MS);
            }
            if (disposed) break;
            await feed.source.add(
              packet.clone({ timestamp: rel }),
              firstMeta ? { decoderConfig: decoderConfig ?? undefined } : undefined,
            );
            firstMeta = false;
            audioFedRel = rel;
          }
        } else {
          const srcRate = await audioTrack.getSampleRate();
          const srcChannels = await audioTrack.getNumberOfChannels();
          if (plan.kind === "transcode" && plan.channels !== srcChannels) {
            throw new Error(
              `live: encoder wants ${plan.channels}ch but source is ${srcChannels}ch`,
            );
          }
          const targetCodec = plan.kind === "transcode" ? plan.targetCodec : "aac";
          const samplesPerFrame = targetCodec === "opus" ? 960 : 1024;
          const frameDurS = samplesPerFrame / srcRate;

          type EncodedOut = { data: Uint8Array; config: AudioDecoderConfig | null };
          const encodedQueue: EncodedOut[] = [];
          let encoderError: Error | null = null;
          let outIndex = 0;
          let firstMeta = true;
          let sentConfig: AudioDecoderConfig | null = null;
          let pumping = false;
          /** Content-rel time of the FIRST PCM entering the encoder — the
           *  origin of the arithmetic output timeline. */
          let contentBase = 0;
          const pumpEncoded = async () => {
            if (pumping) return;
            pumping = true;
            try {
              while (encodedQueue.length > 0 && !disposed) {
                const out = encodedQueue.shift()!;
                if (out.config) sentConfig = out.config;
                const ts = contentBase + outIndex * frameDurS;
                const packet = new EncodedPacket(out.data, "key", ts, frameDurS, outIndex);
                await feed.source.add(
                  packet,
                  firstMeta ? { decoderConfig: sentConfig ?? undefined } : undefined,
                );
                firstMeta = false;
                outIndex += 1;
                audioFedRel = ts;
              }
            } catch (e) {
              encoderError = encoderError ?? (e instanceof Error ? e : new Error(String(e)));
            } finally {
              pumping = false;
            }
          };
          const aEncoder = new AudioEncoder({
            output: (chunk, meta) => {
              const buf = new Uint8Array(chunk.byteLength);
              chunk.copyTo(buf);
              encodedQueue.push({ data: buf, config: meta?.decoderConfig ?? null });
              void pumpEncoded();
            },
            error: (e) => {
              encoderError = e instanceof Error ? e : new Error(String(e));
            },
          });
          aEncoder.configure({
            codec: targetCodec === "aac" ? "mp4a.40.2" : "opus",
            sampleRate: srcRate,
            numberOfChannels: srcChannels,
            bitrate: targetCodec === "opus" ? 128_000 : 192_000,
            ...(targetCodec === "aac" ? { aac: { format: "aac" } } : { opus: { format: "opus" } }),
          } as AudioEncoderConfig);

          // Content-gap reconciliation. The arithmetic output timeline
          // (`contentBase + n × frameDur`) assumes the PCM entering the
          // encoder is CONTIGUOUS — but the eac3 stream drops the odd
          // frame (broadcast glitches, decode skips; 40-120 ms gaps were
          // measured). Every unfilled gap would shift the stamps ahead of
          // the content forever — the accumulating lip-sync drift. So:
          // when the content jumps, encode SILENCE for the missing span,
          // exactly like any real player's audio renderer does.
          let expectedRel: number | null = null;
          const encodeSilence = (seconds: number) => {
            let remain = seconds;
            while (remain > 0.001 && !disposed) {
              const frames = Math.min(1536, Math.max(1, Math.round(remain * srcRate)));
              const pcm = new Float32Array(frames * srcChannels);
              const silent = new AudioData({
                format: "f32",
                sampleRate: srcRate,
                numberOfFrames: frames,
                numberOfChannels: srcChannels,
                timestamp: 0, // encoder stamps are ignored by design
                data: pcm,
              });
              try {
                aEncoder.encode(silent);
              } finally {
                silent.close();
              }
              remain -= frames / srcRate;
            }
          };

          const sampleSink = new AudioSampleSink(audioTrack);
          for await (const sample of sampleSink.samples(anchor, Number.POSITIVE_INFINITY)) {
            if (disposed || encoderError) {
              sample.close();
              break;
            }
            try {
              const rel = Math.max(0, sample.timestamp - anchor);
              while (
                !disposed &&
                (rel - playheadRel() > (video.readyState > 0 ? FEED_AHEAD_S : PRESTART_AHEAD_S) ||
                  rel - videoFedRel > TRACK_LEAD_CAP)
              ) {
                await sleep(PACE_MS);
              }
              if (disposed) break;
              if (expectedRel === null) {
                contentBase = rel;
                expectedRel = rel;
              }
              const gap = rel - expectedRel;
              if (gap > 0.015) {
                console.warn(
                  `[iris-core] live-c: audio content gap ${(gap * 1000).toFixed(0)}ms at ` +
                    `${rel.toFixed(2)}s — filling with silence`,
                );
                encodeSilence(gap);
              }
              expectedRel = rel + sample.duration;
              const data = sample.toAudioData();
              try {
                aEncoder.encode(data);
              } finally {
                data.close();
              }
            } finally {
              sample.close();
            }
          }
          if (encoderError) throw encoderError;
          try {
            await aEncoder.flush();
            await pumpEncoded();
          } catch {
            /* teardown */
          }
          try {
            aEncoder.close();
          } catch {
            /* idempotent */
          }
        }
        try {
          await feed.source.close();
        } catch {
          /* output cancelled mid-flush */
        }
      };
      void feedAudio().catch((e: unknown) => {
        if (!disposed) {
          console.warn("[iris-core] live-c: audio pipeline ended", e);
          fail(e instanceof Error ? e : new Error(String(e)));
        }
      });
    } else {
      audioFedRel = Number.POSITIVE_INFINITY;
    }

    // ---- video: decode (join concealment) → hw re-encode → mux ----------

    let decoderBroken: Error | null = null;
    let encoderBroken: Error | null = null;
    const resetStamps: number[] = [];
    let decOut = 0;
    /** FIFO of rel timestamps of frames fed to the ENCODER — its output
     *  chunks are 1:1 and in order (realtime mode, no B-frames), so the
     *  n-th chunk IS the n-th frame. Encoder stamps are never trusted. */
    const encTsFifo: number[] = [];
    let videoPacketMeta: { decoderConfig?: VideoDecoderConfig } | undefined;
    let vFirstMeta = true;
    let vOutIndex = 0;
    let vPumping = false;
    type VChunkOut = { data: Uint8Array; type: "key" | "delta" };
    const vChunkQueue: VChunkOut[] = [];
    const pumpVideo = async () => {
      if (vPumping) return;
      vPumping = true;
      try {
        while (vChunkQueue.length > 0 && !disposed) {
          const out = vChunkQueue.shift()!;
          const ts = encTsFifo.shift();
          if (ts == null) break; // desynced FIFO — shouldn't happen
          const packet = new EncodedPacket(out.data, out.type, ts, 1 / 50, vOutIndex);
          await videoSrc.add(packet, vFirstMeta ? videoPacketMeta : undefined);
          vFirstMeta = false;
          vOutIndex += 1;
          videoFedRel = ts;
        }
      } catch (e) {
        encoderBroken = encoderBroken ?? (e instanceof Error ? e : new Error(String(e)));
      } finally {
        vPumping = false;
      }
    };

    let framesSinceKey = ENC_GOP_FRAMES; // first encoded frame = IDR
    const vEncoder = new VideoEncoder({
      output: (chunk, meta) => {
        if (meta?.decoderConfig && !videoPacketMeta) {
          videoPacketMeta = { decoderConfig: meta.decoderConfig };
        }
        const buf = new Uint8Array(chunk.byteLength);
        chunk.copyTo(buf);
        vChunkQueue.push({ data: buf, type: chunk.type === "key" ? "key" : "delta" });
        void pumpVideo();
      },
      error: (e) => {
        encoderBroken = e instanceof Error ? e : new Error(String(e));
      },
    });
    vEncoder.configure(encConfig);
    encoder = vEncoder;

    // Stretch-blit stage for decoded frames whose size ≠ the encode target
    // (Firefox emits interlaced content as 1920×540 field frames; encoding
    // them directly letterboxes). Lazily initialised on the first frame.
    let scaleCanvas: OffscreenCanvas | null = null;
    let scaleCtx: OffscreenCanvasRenderingContext2D | null = null;
    let scaleDecided = false;
    let needsScale = false;

    const makeDecoder = (): VideoDecoder => {
      const d = new VideoDecoder({
        output: (frame) => {
          if (disposed) {
            frame.close();
            return;
          }
          decOut += 1;
          const rel = Math.max(0, frame.timestamp / 1_000_000 - anchor);
          try {
            if (!scaleDecided) {
              scaleDecided = true;
              needsScale = frame.displayWidth !== width || frame.displayHeight !== height;
              console.log(
                `[iris-core] live-c: first frame ${frame.codedWidth}x${frame.codedHeight} ` +
                  `(display ${frame.displayWidth}x${frame.displayHeight}) → ` +
                  `${needsScale ? `stretch-blit to ${width}x${height}` : "direct encode"}`,
              );
              if (needsScale) {
                scaleCanvas = new OffscreenCanvas(width, height);
                scaleCtx = scaleCanvas.getContext("2d", { alpha: false });
              }
            }
            const key = framesSinceKey >= ENC_GOP_FRAMES;
            if (key) framesSinceKey = 0;
            framesSinceKey += 1;
            let toEncode: VideoFrame = frame;
            if (needsScale && scaleCanvas && scaleCtx) {
              scaleCtx.drawImage(frame, 0, 0, width, height);
              toEncode = new VideoFrame(scaleCanvas, { timestamp: frame.timestamp });
            }
            try {
              encTsFifo.push(rel);
              vEncoder.encode(toEncode, { keyFrame: key });
            } finally {
              if (toEncode !== frame) toEncode.close();
            }
          } catch (e) {
            encoderBroken = encoderBroken ?? (e instanceof Error ? e : new Error(String(e)));
          } finally {
            frame.close();
          }
        },
        error: (err) => {
          decoderBroken = err instanceof Error ? err : new Error(String(err));
        },
      });
      d.configure(configWithFreshDescription(videoConfig));
      return d;
    };
    decoder = makeDecoder();

    const videoP = (async () => {
      let awaitingKey = false;
      let fedMax = anchor;
      let lastLogged = anchor;
      for await (const packet of videoPacketSink.packets(startPacket)) {
        if (disposed) break;
        if (encoderBroken) {
          fail(encoderBroken);
          return;
        }
        while (!disposed && decoderBroken == null) {
          const d = decoder;
          if (!d || d.state === "closed") break;
          const rel = packet.timestamp - anchor;
          const tooDeep = d.decodeQueueSize > 8 || vEncoder.encodeQueueSize > 8;
          const tooFar =
            rel - audioFedRel > TRACK_LEAD_CAP ||
            rel - playheadRel() > (playbackStarted ? FEED_AHEAD_S : PRESTART_AHEAD_S);
          if (!tooDeep && !tooFar) break;
          await sleep(PACE_MS);
        }
        if (disposed) break;

        if (decoderBroken) {
          const now = Date.now();
          while (resetStamps.length > 0 && now - resetStamps[0]! > RESET_WINDOW_MS) {
            resetStamps.shift();
          }
          if (resetStamps.length >= RESET_BUDGET) {
            fail(
              new Error(
                `live: ${RESET_BUDGET} decoder resets in ${RESET_WINDOW_MS / 1000}s — ${decoderBroken.message}`,
              ),
            );
            return;
          }
          resetStamps.push(now);
          console.warn(
            `[iris-core] live-c: decoder reset #${resetStamps.length} at ` +
              `${packet.timestamp.toFixed(1)}s (${decoderBroken.message}) — rejoining at next keyframe`,
          );
          decoderBroken = null;
          try {
            decoder?.close();
          } catch {
            /* already closed */
          }
          decoder = makeDecoder();
          awaitingKey = true;
        }
        if (awaitingKey && packet.type !== "key") continue;
        awaitingKey = false;

        try {
          decoder.decode(packet.toEncodedVideoChunk());
        } catch (e) {
          decoderBroken = decoderBroken ?? (e instanceof Error ? e : new Error(String(e)));
        }
        if (packet.timestamp > fedMax) fedMax = packet.timestamp;
        if (fedMax - lastLogged >= 5) {
          lastLogged = fedMax;
          console.log(
            `[iris-core] live-c: fed v=${videoFedRel.toFixed(1)}s a=${audioFedRel === Number.POSITIVE_INFINITY ? "-" : audioFedRel.toFixed(1)}s ` +
              `t=${video.currentTime.toFixed(1)}s dec=${decOut} encQ=${vEncoder.encodeQueueSize} ` +
              `resets=${resetStamps.length} aQ=${appendQueue.length} rs=${video.readyState} ` +
              `buf=${sourceBuffer && sourceBuffer.buffered.length > 0 ? (sourceBuffer.buffered.end(sourceBuffer.buffered.length - 1) - video.currentTime).toFixed(1) : "-"}s`,
          );
        }
      }
      try {
        await vEncoder.flush();
      } catch {
        /* teardown */
      }
      try {
        await videoSrc.close();
      } catch {
        /* output cancelled mid-flush */
      }
      // ENDLIST — backend session died; the page rotates sources.
      if (!disposed) opts.onEnded?.();
    })();
    void videoP;
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
