/**
 * Tier C / D — WebCodecs decode + canvas render + Web Audio.
 *
 * "Bypass MSE entirely": Mediabunny demux → `VideoDecoder` /
 * `AudioDecoder` → renderer (Canvas2D today, WebGPU when available).
 * Audio scheduler is the master clock; renderer chases it.
 *
 * Implements full `EngineHandle`:
 *   - `play` / `pause` — pause the AV master clock (audio gain → 0,
 *     decoder loops paused via a flag observed by the pipelines).
 *   - `seek` — drain decoders, reset scheduler clock, re-spin
 *     pipelines from a new keyframe. Frame-accurate to the nearest
 *     key packet.
 *   - `setVolume` / `setMuted` — scheduler `GainNode`.
 */

import {
  ALL_FORMATS,
  Input,
  UrlSource,
  type InputAudioTrack,
  type InputVideoTrack,
} from "mediabunny";

import { startAudioPipeline, type AudioPipelineHandle } from "../decode/audio-pipeline";
import { startVideoPipeline, type VideoPipelineHandle } from "../decode/video-pipeline";
import { probeVideoTrack } from "../decode/webcodecs-probe";
import { createAudioScheduler, type AudioScheduler } from "../audio/audio-scheduler";
import { mountRenderer, type VideoRenderer } from "../render/renderer-factory";
import type { EngineAudioTrack, EngineHandle, EngineMount } from "../engine";

export const mountTierC: EngineMount = async (opts) => {
  const { container, streamUrl, startPosition, audioTrackIndex } = opts;
  const onError = opts.onError;

  container.innerHTML = "";

  const input = new Input({
    source: new UrlSource(streamUrl, { requestInit: { credentials: "include" } }),
    formats: ALL_FORMATS,
  });

  const videoTrack = (await input.getPrimaryVideoTrack()) ?? null;
  if (!videoTrack) {
    const err = new Error("Tier C: no primary video track");
    onError(err);
    throw err;
  }
  const probe = await probeVideoTrack(videoTrack);
  if (!probe || !probe.decodes) {
    const err = new Error("Tier C probe: decoder rejected the keyframe");
    try {
      await input.dispose();
    } catch {
      /* idempotent */
    }
    onError(err);
    throw err;
  }

  // Pick the audio track. The chrome's audio picker writes
  // `audioTrackIndex` (= position in `manifest.audio`); we walk
  // Mediabunny's input.getAudioTracks() and take that index.
  // Defaults to the primary track when no index is supplied.
  const allAudio = await input.getAudioTracks();
  const audioTrack =
    audioTrackIndex != null && audioTrackIndex >= 0 && audioTrackIndex < allAudio.length
      ? allAudio[audioTrackIndex] ?? null
      : (await input.getPrimaryAudioTrack()) ?? null;
  const audioConfig = audioTrack ? await audioTrack.getDecoderConfig() : null;

  const scheduler: AudioScheduler = await createAudioScheduler();
  const renderer: VideoRenderer = await mountRenderer({
    container,
    clockSeconds: () => scheduler.currentMediaTimeSeconds(),
    hdr: probe.config.codec?.startsWith("hev1") || probe.config.codec?.startsWith("hvc1") ? "auto" : "sdr",
    onError,
  });

  let readyFired = false;
  const fireReady = () => {
    if (readyFired) return;
    readyFired = true;
    opts.onReady?.();
  };

  // Pipeline state that survives across seeks.
  let videoHandle: VideoPipelineHandle | null = null;
  let audioHandle: AudioPipelineHandle | null = null;
  let currentSeekTarget = startPosition;
  let seekGeneration = 0;
  let paused = false;
  let disposed = false;

  const spinPipelines = (fromSeconds: number, generation: number): void => {
    videoHandle = startVideoPipeline({
      track: videoTrack as InputVideoTrack,
      config: probe.config,
      startSeconds: fromSeconds,
      onFrame: (frame) => {
        if (generation !== seekGeneration || disposed) {
          frame.close();
          return;
        }
        renderer.enqueue(frame);
        fireReady();
      },
      onError,
      onEnd: () => {
        if (generation === seekGeneration) opts.onEnded?.();
      },
    });
    if (audioTrack && audioConfig) {
      audioHandle = startAudioPipeline({
        track: audioTrack as InputAudioTrack,
        config: audioConfig,
        startSeconds: fromSeconds,
        onData: (data) => {
          if (generation !== seekGeneration || disposed) {
            data.close();
            return;
          }
          scheduler.enqueue(data);
        },
        onError,
      });
    }
  };

  spinPipelines(startPosition, seekGeneration);

  // 4 Hz time-update broadcast so the parent can save resume position
  // and the chrome can update its display.
  const tickInterval = setInterval(() => {
    opts.onTimeUpdate?.(currentMediaTime());
  }, 250);

  // The scheduler's clock is anchored to absolute media time on first
  // enqueue post-reset (data.timestamp is absolute, not seek-relative).
  // So `currentMediaTimeSeconds()` already returns the right value.
  const currentMediaTime = (): number => {
    if (audioTrack) return scheduler.currentMediaTimeSeconds();
    return currentSeekTarget;
  };

  const handle: EngineHandle = {
    dispose: async () => {
      disposed = true;
      clearInterval(tickInterval);
      await Promise.allSettled([
        videoHandle?.stop() ?? Promise.resolve(),
        audioHandle?.stop() ?? Promise.resolve(),
      ]);
      renderer.dispose();
      await scheduler.dispose();
      try {
        await input.dispose();
      } catch {
        /* idempotent */
      }
    },
    currentTime: () => currentMediaTime(),
    duration: () => opts.manifest.duration_s,
    paused: () => paused,
    volume: () => scheduler.getVolume(),
    muted: () => scheduler.getMuted(),
    buffered: () => [],
    play: async () => {
      if (!paused) return;
      paused = false;
      scheduler.setMuted(false);
      opts.onPlayingChange?.(true);
    },
    pause: () => {
      if (paused) return;
      paused = true;
      scheduler.setMuted(true);
      opts.onPlayingChange?.(false);
      opts.onPause?.(currentMediaTime());
    },
    seek: (seconds: number) => {
      // Tier C seek: drain → reset scheduler → re-spin pipelines from
      // the closest preceding keyframe. Frame-accurate to the nearest
      // key packet. The bumped generation ID makes any in-flight
      // decode output from the previous run get dropped on arrival.
      const target = Math.max(0, seconds);
      seekGeneration += 1;
      currentSeekTarget = target;
      const gen = seekGeneration;
      const prevVideo = videoHandle;
      const prevAudio = audioHandle;
      videoHandle = null;
      audioHandle = null;
      void (async () => {
        await Promise.allSettled([
          prevVideo?.stop() ?? Promise.resolve(),
          prevAudio?.stop() ?? Promise.resolve(),
        ]);
        if (disposed || gen !== seekGeneration) return;
        scheduler.resetClock();
        opts.onSeeking?.(target);
        spinPipelines(target, gen);
      })();
    },
    setVolume: (v) => scheduler.setVolume(v),
    setMuted: (m) => scheduler.setMuted(m),
    audioTracks: (): EngineAudioTrack[] => {
      const defaultIdx = Math.max(0, opts.manifest.audio.findIndex((x) => x.default));
      const activeIdx = audioTrackIndex ?? defaultIdx;
      return opts.manifest.audio.map((a, i) => ({
        id: String(i),
        label: a.title ?? a.lang?.toUpperCase() ?? `Audio ${i + 1}`,
        lang: a.lang ?? undefined,
        active: i === activeIdx,
      }));
    },
    // Tier C audio switch needs a remount (the decoder is bound to a
    // single Mediabunny track). `IrisPlayer` triggers that via the
    // mount-key including `audioTrackIndex`.
    setAudioTrack: () => undefined,
    setNativeSubtitle: () => undefined,
    videoElement: () => null,
    canvasElement: () => renderer.canvas,
  };
  return handle;
};
