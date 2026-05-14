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
  /** Index into `manifest.audio` (NOT `stream_idx`) for the audio track
   *  the engine should activate on mount. `undefined` = engine picks
   *  its own default (typically the file's primary). Tier B/C react
   *  to this; Tier F switches audio via its handle's `setAudioTrack`
   *  (no remount needed); Tier A is single-audio. Changing this value
   *  triggers a remount in `IrisPlayer`. */
  audioTrackIndex?: number;

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

  // Native subtitles -------------------------------------------------
  /** Set the active native (`<track>`-renderable) subtitle by the
   *  `stream_idx` it had in the manifest. `null` disables all native
   *  subs. Engines without a `<video>` element are a no-op — ASS/PGS
   *  overlay paths run from `IrisPlayer` instead. */
  setNativeSubtitle: (streamIdx: number | null) => void;

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
 *  `<video>`. The `trackMap` ties manifest `stream_idx` → live
 *  `HTMLTrackElement` so the chrome can flip native subs on/off by
 *  the same identity it uses in the picker menu. */
export function videoBackedHandle(
  video: HTMLVideoElement,
  extras: {
    dispose: () => Promise<void>;
    audioTracks?: () => EngineAudioTrack[];
    setAudioTrack?: (id: string) => void;
    /** Map stream_idx → the `<track>` element this engine injected
     *  for it. Tier A/B/F engines build this when creating their
     *  per-sub `<track>` elements. */
    nativeTrackMap?: Map<number, HTMLTrackElement>;
    /** Fallback duration the handle reports when `video.duration` is
     *  `Infinity` (MSE before `endOfStream`) or NaN. The chrome
     *  needs a finite number to draw the scrub bar. */
    fallbackDuration?: number | null;
  },
): EngineHandle {
  return {
    dispose: extras.dispose,
    currentTime: () => video.currentTime,
    duration: () => {
      if (Number.isFinite(video.duration) && video.duration > 0) return video.duration;
      return extras.fallbackDuration ?? null;
    },
    paused: () => video.paused,
    volume: () => video.volume,
    muted: () => video.muted,
    buffered: () => {
      // Defensive: Firefox can throw `DOMException: Index or size
      // is negative…` from `<video>.buffered.start/end(i)` when the
      // media element is in a transient state (mid-detach, after a
      // decode error, …). The TimeRanges length is read fresh on
      // each iteration so most races resolve themselves, but a
      // throw here would propagate into the chrome's rAF tick →
      // `setBuffered` during render → React errors recursively
      // through the tree (visible as a long stack of minified
      // function frames repeating). Catching keeps the chrome
      // responsive even when the underlying media is wedged.
      const ranges: Array<[number, number]> = [];
      try {
        const tr = video.buffered;
        for (let i = 0; i < tr.length; i += 1) {
          ranges.push([tr.start(i), tr.end(i)]);
        }
      } catch {
        /* return whatever we managed to collect so far */
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
    setNativeSubtitle: (streamIdx) => {
      if (!extras.nativeTrackMap) return;
      // Try to apply the mode change. The `TextTrack` backing each
      // `<track>` element is created lazily by the browser — `el.track`
      // can be null for a few rAF ticks after `appendChild`. Retry up
      // to ~1 s before giving up so the picker selection actually takes.
      const trackMap = extras.nativeTrackMap;
      let attempts = 0;
      const apply = (): void => {
        let pending = false;
        for (const [idx, trackEl] of trackMap) {
          const t = trackEl.track;
          if (!t) {
            pending = true;
            continue;
          }
          t.mode = idx === streamIdx ? "showing" : "disabled";
        }
        if (pending && attempts < 60) {
          attempts += 1;
          requestAnimationFrame(apply);
        }
      };
      apply();
    },
    videoElement: () => video,
    canvasElement: () => null,
  };
}

/** Reusable helper: build a `<track>` element for a native subtitle
 *  and record it in a `stream_idx → element` map for later
 *  enable/disable by the unified subtitle picker.
 *
 *  We intentionally DO NOT set `el.default` — the unified picker is
 *  the single source of truth for which subtitle is active. Leaving
 *  the browser to auto-show a "default" track would fight the
 *  picker's selection on first frame. */
export function appendNativeTrack(
  video: HTMLVideoElement,
  sub: NativeSubtitleTrack,
  trackMap: Map<number, HTMLTrackElement>,
): HTMLTrackElement {
  const el = document.createElement("track");
  el.src = sub.vttUrl;
  el.kind = "subtitles";
  el.label = sub.title ?? sub.lang?.toUpperCase() ?? `Sub ${sub.stream_idx}`;
  el.srclang = sub.lang ?? "und";
  video.appendChild(el);
  trackMap.set(sub.stream_idx, el);
  return el;
}
