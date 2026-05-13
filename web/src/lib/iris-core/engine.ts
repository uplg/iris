/**
 * Shared engine interface implemented by every tier (A, B, C, D, F).
 *
 * `IrisPlayer` is engine-agnostic: it mounts whichever engine the
 * tier-decision picked, then drives playback through this handle.
 * `IrisChrome` (the controls layer) is similarly engine-agnostic — it
 * only sees `EngineHandle`, not `<video>` or `<canvas>`.
 *
 * Engines that wrap a real `<video>` element (A/B/F) implement most of
 * the API by delegating to `HTMLVideoElement`. The WebCodecs engine
 * (C/D) keeps its own clock + decoder pipeline state.
 */

import type { Manifest, SubtitleTrack } from "./manifest-client";

/** Native `<track>`-renderable subtitle with the URL rewritten to `.vtt`. */
export type NativeSubtitleTrack = SubtitleTrack & { vttUrl: string };

export type EngineAudioTrack = {
  /** Stable identifier — engine-specific (HLS rendition group id,
   *  manifest stream_idx, …). */
  id: string;
  label: string;
  lang?: string;
  active: boolean;
};

export type EngineMountOptions = {
  /** The `<div>` (or any block-level element) the engine renders into. */
  container: HTMLDivElement;
  manifest: Manifest;
  /** `/api/torrents/.../stream` (A/B/C/D) or `/play/master.m3u8` (F). */
  streamUrl: string;
  /** Resume position in seconds; engine seeks here on first decodable frame. */
  startPosition: number;
  /** Native `<track>`-renderable subtitle tracks. The URL is already
   *  rewritten to the `.vtt` endpoint. ASS / PGS subs never appear here. */
  nativeSubs: NativeSubtitleTrack[];

  onReady?: () => void;
  /** Fires on `timeupdate` (native) or on the master-clock tick (C/D). */
  onTimeUpdate?: (mediaTimeSeconds: number) => void;
  onDurationChange?: (durationSeconds: number) => void;
  onPlayingChange?: (playing: boolean) => void;
  onSeeking?: (mediaTimeSeconds: number) => void;
  onPause?: (mediaTimeSeconds: number) => void;
  onEnded?: () => void;
  onAudioTracksChange?: (tracks: EngineAudioTrack[]) => void;
  /** Terminal failure: caller treats this as a demotion signal. */
  onError: (err: Error) => void;
};

export type EngineHandle = {
  // Lifecycle ---------------------------------------------------------
  dispose: () => Promise<void>;

  // Read state --------------------------------------------------------
  currentTime: () => number;
  duration: () => number | null;
  paused: () => boolean;
  volume: () => number;
  muted: () => boolean;
  /** Buffered byte/time ranges as `[start, end]` pairs in seconds. */
  buffered: () => Array<[number, number]>;

  // Controls ---------------------------------------------------------
  play: () => Promise<void>;
  pause: () => void;
  /** Seek to `seconds`. Engines that can't seek (e.g., Tier C without
   *  re-mount logic) should still attempt it and surface a warning. */
  seek: (seconds: number) => void;
  setVolume: (vol01: number) => void;
  setMuted: (muted: boolean) => void;

  // Audio tracks -----------------------------------------------------
  audioTracks: () => EngineAudioTrack[];
  setAudioTrack: (id: string) => void;

  // Optional escape hatches ------------------------------------------
  /** The underlying `<video>` element when the engine has one. Used by
   *  `IrisChrome` for native fullscreen + Document PiP wiring. Returns
   *  null for canvas-only engines (C/D). */
  videoElement: () => HTMLVideoElement | null;
  /** The underlying `<canvas>` element when the engine renders to a
   *  canvas (C/D). Used by `IrisChrome` to wire Document PiP via a
   *  captured MediaStream. Null otherwise. */
  canvasElement: () => HTMLCanvasElement | null;
};

export type EngineMount = (opts: EngineMountOptions) => Promise<EngineHandle>;

/** Convenience: build the standard set of `<video>` event listeners
 *  that forward to the unified callbacks. Engines that wrap a `<video>`
 *  (A/B/F) all use this. */
export function bindVideoCallbacks(
  video: HTMLVideoElement,
  opts: EngineMountOptions,
  initialSeek: { done: boolean },
): () => void {
  const onTime = () => opts.onTimeUpdate?.(video.currentTime);
  const onDuration = () => {
    if (Number.isFinite(video.duration) && video.duration > 0) {
      opts.onDurationChange?.(video.duration);
    }
  };
  const onSeek = () => opts.onSeeking?.(video.currentTime);
  const onPause = () => {
    opts.onPause?.(video.currentTime);
    opts.onPlayingChange?.(false);
  };
  const onPlaying = () => opts.onPlayingChange?.(true);
  const onEnded = () => opts.onEnded?.();
  const onCanPlay = () => {
    if (initialSeek.done) return;
    initialSeek.done = true;
    if (opts.startPosition > 0) {
      try {
        video.currentTime = opts.startPosition;
      } catch {
        /* swallow */
      }
    }
  };
  video.addEventListener("timeupdate", onTime);
  video.addEventListener("durationchange", onDuration);
  video.addEventListener("seeking", onSeek);
  video.addEventListener("pause", onPause);
  video.addEventListener("playing", onPlaying);
  video.addEventListener("ended", onEnded);
  video.addEventListener("canplay", onCanPlay);
  return () => {
    video.removeEventListener("timeupdate", onTime);
    video.removeEventListener("durationchange", onDuration);
    video.removeEventListener("seeking", onSeek);
    video.removeEventListener("pause", onPause);
    video.removeEventListener("playing", onPlaying);
    video.removeEventListener("ended", onEnded);
    video.removeEventListener("canplay", onCanPlay);
  };
}

/** Build a standard `<video>` handle backed by an `HTMLVideoElement`.
 *  Used by Tier A/B/F. Tier C overrides most of this since it has no
 *  `<video>`. */
export function videoBackedHandle(
  video: HTMLVideoElement,
  extras: {
    dispose: () => Promise<void>;
    audioTracks?: () => EngineAudioTrack[];
    setAudioTrack?: (id: string) => void;
  },
): EngineHandle {
  return {
    dispose: extras.dispose,
    currentTime: () => video.currentTime,
    duration: () =>
      Number.isFinite(video.duration) && video.duration > 0 ? video.duration : null,
    paused: () => video.paused,
    volume: () => video.volume,
    muted: () => video.muted,
    buffered: () => {
      const ranges: Array<[number, number]> = [];
      for (let i = 0; i < video.buffered.length; i += 1) {
        ranges.push([video.buffered.start(i), video.buffered.end(i)]);
      }
      return ranges;
    },
    play: () => video.play(),
    pause: () => video.pause(),
    seek: (s) => {
      try {
        video.currentTime = s;
      } catch {
        /* swallow */
      }
    },
    setVolume: (v) => {
      video.volume = Math.max(0, Math.min(1, v));
    },
    setMuted: (m) => {
      video.muted = m;
    },
    audioTracks: extras.audioTracks ?? (() => []),
    setAudioTrack: extras.setAudioTrack ?? (() => undefined),
    videoElement: () => video,
    canvasElement: () => null,
  };
}
