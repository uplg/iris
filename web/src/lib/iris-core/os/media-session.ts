/**
 * Media Session API wiring. Surfaces playback metadata + transport
 * controls to the OS (lock screen, media keys, AirPods/Bluetooth
 * remote-press, Linux MPRIS).
 *
 * Tier-agnostic: routes everything through `EngineHandle`, so canvas-
 * only Tier C still gets OS integration that the legacy `<video>` PiP
 * couldn't provide.
 */

import type { EngineHandle } from "../engine";
import type { Manifest } from "../manifest-client";

export type MediaSessionWiring = {
  dispose: () => void;
};

export function attachMediaSession(
  handle: EngineHandle,
  manifest: Manifest,
  meta: { title: string; artwork?: MediaImage[] },
): MediaSessionWiring {
  if (typeof navigator === "undefined" || !("mediaSession" in navigator)) {
    return { dispose: () => undefined };
  }
  const ms = navigator.mediaSession;

  ms.metadata = new MediaMetadata({
    title: meta.title,
    artist: manifest.filename,
    album: "Iris",
    artwork: meta.artwork ?? [],
  });

  const actions: Array<[MediaSessionAction, MediaSessionActionHandler]> = [
    ["play", () => void handle.play()],
    ["pause", () => handle.pause()],
    [
      "seekto",
      (e) => {
        if (typeof e.seekTime === "number") handle.seek(e.seekTime);
      },
    ],
    [
      "seekbackward",
      (e) => {
        const step = typeof e.seekOffset === "number" ? e.seekOffset : 10;
        handle.seek(Math.max(0, handle.currentTime() - step));
      },
    ],
    [
      "seekforward",
      (e) => {
        const step = typeof e.seekOffset === "number" ? e.seekOffset : 10;
        handle.seek(handle.currentTime() + step);
      },
    ],
    ["stop", () => handle.pause()],
  ];
  for (const [action, fn] of actions) {
    try {
      ms.setActionHandler(action, fn);
    } catch {
      // Some actions are platform-gated (e.g., `seekto` requires the
      // browser to know duration). Silently skip unsupported ones.
    }
  }

  // Position-state ticks at 4 Hz so the OS scrubber tracks the actual
  // playhead. setPositionState was added incrementally; guard the call.
  let positionTimer: ReturnType<typeof setInterval> | null = null;
  const updatePosition = () => {
    const duration = handle.duration();
    if (duration == null || duration <= 0) return;
    try {
      ms.setPositionState({
        duration,
        playbackRate: 1,
        position: Math.max(0, Math.min(duration, handle.currentTime())),
      });
    } catch {
      /* unsupported: noop */
    }
  };
  positionTimer = setInterval(updatePosition, 250);

  const playbackPoll = setInterval(() => {
    ms.playbackState = handle.paused() ? "paused" : "playing";
  }, 500);

  return {
    dispose: () => {
      if (positionTimer) clearInterval(positionTimer);
      clearInterval(playbackPoll);
      try {
        ms.metadata = null;
        for (const [action] of actions) ms.setActionHandler(action, null);
        ms.playbackState = "none";
      } catch {
        /* idempotent */
      }
    },
  };
}
