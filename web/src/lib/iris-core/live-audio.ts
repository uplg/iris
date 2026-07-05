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

/** Target A/V offset: schedule audio this far ahead of "now" so there is
 *  headroom before the buffer must play. Small enough to stay in sync. */
const SCHEDULE_LEAD_S = 0.35;

/** Re-anchor (accepting a tiny discontinuity) when the contiguous schedule
 *  drifts from the video's PDT by more than this. */
const RESYNC_THRESHOLD_S = 0.3;

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

  // `scheduledUntil` is the context-clock time up to which audio is booked.
  // Contiguous scheduling avoids clicks; we only jump it on a real drift.
  let scheduledUntil = 0;

  /** Current video PDT (seconds) — the shared wall clock. Null until hls.js
   *  has a program-date-time and is actually playing. */
  const videoPdt = (): number | null => {
    const d = hls.playingDate;
    return d ? d.getTime() / 1000 : null;
  };

  void (async () => {
    try {
      for await (const sample of new AudioSampleSink(audioTrack).samples(
        0,
        Number.POSITIVE_INFINITY,
      )) {
        if (disposed) {
          sample.close();
          return;
        }
        try {
          const pdt = videoPdt();
          if (pdt === null) continue; // video not started yet — skip until it is
          const buffer = sample.toAudioBuffer();
          const samplePdt = sample.timestamp; // epoch seconds (from PDT)
          const now = ctx.currentTime;
          // Where this sample SHOULD play so its PDT matches the video's.
          const idealStart = now + (samplePdt - pdt) + SCHEDULE_LEAD_S;

          let startAt: number;
          if (needAnchor || scheduledUntil < now || Math.abs(scheduledUntil - idealStart) > RESYNC_THRESHOLD_S) {
            // (Re)anchor to the video clock.
            startAt = Math.max(idealStart, now + 0.02);
            needAnchor = false;
          } else {
            startAt = scheduledUntil; // contiguous with the previous buffer
          }
          if (startAt + buffer.duration <= now) continue; // hopelessly late — drop

          const node = ctx.createBufferSource();
          node.buffer = buffer;
          node.connect(gain);
          node.addEventListener("ended", () => activeNodes.delete(node));
          activeNodes.add(node);
          node.start(startAt);
          scheduledUntil = startAt + buffer.duration;
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
      dispose();
    },
  };
}
