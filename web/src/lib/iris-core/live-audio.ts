/**
 * Live-TV E-AC-3 audio sidecar.
 *
 * hls.js plays the VIDEO robustly (it is battle-tested against dirty
 * broadcast restreams) but drops audio codecs MSE can't decode — E-AC-3 /
 * AC-3, which browsers have no license for — leaving silent video. Rather
 * than remux the whole stream (fragile: the muxer + WebCodecs interaction
 * never finalises fragments on Firefox), we leave video to hls.js and decode
 * ONLY the audio here: a second mediabunny read of the same playlist demuxes
 * the audio ES, the registered libav.js `CustomAudioDecoder` (the exact one
 * the VOD player uses) decodes E-AC-3 → PCM, and we play it through WebAudio,
 * scheduled against the video's clock.
 *
 * Sync anchor: both the decoded audio samples and hls.js expose a wall-clock
 * time derived from the stream's `EXT-X-PROGRAM-DATE-TIME` — the audio
 * sample's `timestamp` (epoch seconds) and `hls.playingDate` (a Date). We
 * schedule each PCM buffer so its PDT lines up with the video's current PDT.
 */

import type Hls from "hls.js";
import { ALL_FORMATS, AudioSampleSink, Input, UrlSource } from "mediabunny";

import { ensureLibavAudioDecoderRegistered } from "./decode/libav-audio-decoder";

/** Minimum headroom: never schedule a buffer to start closer than this to
 *  "now" (WebAudio needs a beat of lead to start a source cleanly). */
const MIN_LEAD_S = 0.05;

/** Allowed A/V drift before we correct. Audio LAGGING the video past this is
 *  fixed by DROPPING buffers to catch up; audio LEADING past it is fixed by
 *  PADDING (a silent gap). We never rewind the play head, so two buffers can
 *  never overlap — the cause of the audible doubling/echo. */
const MAX_DRIFT_S = 0.12;

/** Cap on how far ahead of the audio clock we pre-schedule buffers. The
 *  decoder races to the live edge while hls.js plays ~one live-latency window
 *  behind it, so without a cap we'd queue the whole window (hundreds of nodes)
 *  up front. We throttle the decode loop to stay within this of playback. */
const MAX_LOOKAHEAD_S = 4;

export interface LiveAudioHandle {
  dispose(): void;
}

/**
 * Start decoding + playing the stream's E-AC-3/AC-3 audio through WebAudio,
 * synced to `video` (driven by `hls`). No-op handle if the stream has no
 * audio track. Never throws into the caller — audio is best-effort; a
 * failure just leaves the video silent.
 */
export async function mountLiveAudio(
  video: HTMLVideoElement,
  hls: Hls,
  masterUrl: string,
): Promise<LiveAudioHandle> {
  ensureLibavAudioDecoderRegistered();

  let disposed = false;
  // Set once the throttle is wired; dispose() calls it so a parked decode
  // loop unblocks (its guard then sees `disposed`) instead of leaking.
  let releaseThrottle: (() => void) | null = null;
  const input = new Input({
    formats: ALL_FORMATS,
    source: new UrlSource(masterUrl, { requestInit: { credentials: "include" } }),
  });
  const ctx = new AudioContext();
  const gain = ctx.createGain();
  gain.connect(ctx.destination);
  const activeNodes = new Set<AudioBufferSourceNode>();

  const dispose = () => {
    if (disposed) return;
    disposed = true;
    releaseThrottle?.();
    for (const n of activeNodes) {
      try {
        n.stop();
      } catch {
        /* already stopped */
      }
    }
    activeNodes.clear();
    void input.dispose?.();
    void ctx.close().catch(() => {});
    resumeCleanup();
  };

  // AudioContext often starts "suspended" without a user gesture. The
  // channel navigation IS a gesture, so resume works; if not, resume on the
  // next interaction (event-driven, no timers).
  const resumeCtx = () => void ctx.resume().catch(() => {});
  resumeCtx();
  const onInteract = () => resumeCtx();
  window.addEventListener("pointerdown", onInteract, { passive: true });
  window.addEventListener("keydown", onInteract, { passive: true });
  const resumeCleanup = () => {
    window.removeEventListener("pointerdown", onInteract);
    window.removeEventListener("keydown", onInteract);
  };

  // Follow the video's play state so audio pauses/resumes with it. Suspending
  // the context freezes its clock, holding the scheduled buffers in place;
  // on resume we force a re-anchor (the video may have jumped to the live
  // edge after a stall).
  let needAnchor = true;
  const onPlaying = () => {
    needAnchor = true;
    void ctx.resume().catch(() => {});
  };
  const onStall = () => void ctx.suspend().catch(() => {});
  video.addEventListener("playing", onPlaying);
  video.addEventListener("pause", onStall);
  video.addEventListener("waiting", onStall);

  const audioTrack = (await input.getAudioTracks())[0] ?? null;
  if (!audioTrack || disposed) {
    if (disposed) dispose();
    return { dispose };
  }

  // We're providing the audio — mute the video element so any codec the
  // browser CAN decode natively (a mixed / fallback source) doesn't play on
  // top of the sidecar and double it.
  video.muted = true;

  // `playHead` is the context-clock time where the next buffer is booked. It
  // only ever moves FORWARD (contiguous, dropped, or padded) — never
  // rewound — so scheduled buffers can never overlap.
  let playHead = 0;

  // Throttle the decode loop when it runs too far ahead of playback, without
  // a timer: park until a scheduled buffer finishes (buffers end ~every 20 ms
  // during playback, so this paces the loop to real time; while the context
  // is suspended nothing ends, which correctly blocks decoding).
  const drainWaiters = new Set<() => void>();
  const throttle = (): Promise<void> => {
    if (disposed || playHead - ctx.currentTime <= MAX_LOOKAHEAD_S) return Promise.resolve();
    return new Promise<void>((resolve) => drainWaiters.add(resolve));
  };
  const onDrain = () => {
    for (const w of drainWaiters) w();
    drainWaiters.clear();
  };
  releaseThrottle = onDrain;

  /** Current video PDT (seconds) — the shared wall clock (from the stream's
   *  PROGRAM-DATE-TIME). Null until hls.js is actually playing. */
  const videoPdt = (): number | null => {
    const d = hls.playingDate;
    return d ? d.getTime() / 1000 : null;
  };

  /** Resolve once the video is playing and exposes a PDT — event-driven, no
   *  timers. We anchor the audio decode to THAT wall-clock position (the live
   *  edge) rather than timestamp 0: starting at 0 would decode from the
   *  oldest segment in the window, hammering already-expired segments (a
   *  404 storm) and running permanently behind the video. */
  const firstVideoPdt = await new Promise<number | null>((resolve) => {
    const p = videoPdt();
    if (p !== null) {
      resolve(p);
      return;
    }
    const onTick = () => {
      if (disposed) {
        cleanup();
        resolve(null);
        return;
      }
      const v = videoPdt();
      if (v !== null) {
        cleanup();
        resolve(v);
      }
    };
    const cleanup = () => {
      video.removeEventListener("timeupdate", onTick);
      video.removeEventListener("playing", onTick);
    };
    video.addEventListener("timeupdate", onTick);
    video.addEventListener("playing", onTick);
  });
  if (disposed || firstVideoPdt === null) {
    dispose();
    return { dispose };
  }

  void (async () => {
    try {
      // Start a hair behind the live edge for a little decode headroom.
      const startFrom = Math.max(0, firstVideoPdt - MIN_LEAD_S);
      for await (const sample of new AudioSampleSink(audioTrack).samples(
        startFrom,
        Number.POSITIVE_INFINITY,
      )) {
        if (disposed) {
          sample.close();
          return;
        }
        await throttle(); // stay within MAX_LOOKAHEAD of playback
        if (disposed) {
          sample.close();
          return;
        }
        try {
          const pdt = videoPdt();
          if (pdt === null) continue; // video not started yet — skip until it is
          const samplePdt = sample.timestamp; // epoch seconds (from PDT)
          const now = ctx.currentTime;
          // Context-clock time at which this sample should be AUDIBLE so its
          // PDT lines up with the video's current PDT.
          const desired = now + (samplePdt - pdt);

          // (Re)anchor after a stall / on first buffer, or if the play head
          // has fallen into the past (context ran while we had nothing).
          if (needAnchor || playHead < now + MIN_LEAD_S) {
            playHead = Math.max(desired, now + MIN_LEAD_S);
            needAnchor = false;
          }

          const drift = playHead - desired;
          if (drift > MAX_DRIFT_S) {
            // Audio is scheduled LATER than the video wants it (audio lagging)
            // — drop this frame to catch up. Not advancing playHead shrinks
            // the drift; a dropped ~32 ms frame is inaudible.
            continue;
          }
          if (drift < -MAX_DRIFT_S) {
            // Audio is scheduled EARLIER than the video (audio leading) — jump
            // the play head forward to the video's position (a brief silent
            // gap) rather than overlapping the previous buffer.
            playHead = desired;
          }

          const node = ctx.createBufferSource();
          node.buffer = sample.toAudioBuffer();
          node.connect(gain);
          node.addEventListener("ended", () => {
            activeNodes.delete(node);
            onDrain(); // a buffer finished — let the throttled loop proceed
          });
          activeNodes.add(node);
          node.start(playHead);
          playHead += sample.duration;
        } finally {
          sample.close();
        }
      }
    } catch (e) {
      if (!disposed) console.warn("[live-audio] decode/playback ended", e);
    }
  })();

  return {
    dispose: () => {
      video.removeEventListener("playing", onPlaying);
      video.removeEventListener("pause", onStall);
      video.removeEventListener("waiting", onStall);
      // Restore native audio for whatever plays next on this element (e.g.
      // a source switch to an AAC feed that needs no sidecar).
      video.muted = false;
      dispose();
    },
  };
}
