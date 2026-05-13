/**
 * Polymorphic subtitle overlay. Picks the right renderer based on the
 * track's codec:
 *
 * - WebVTT / text codecs → returns `null`. The host engine handles
 *   them via the native `<track>` element (Vidstack does this for
 *   Tier A/F; Tier B injects `<track>` children inside its own
 *   `<video>`).
 * - ASS / SSA → mount `mountAssOverlay`.
 * - PGS / DVB / DVD bitmap → mount `mountPgsOverlay`.
 *
 * The overlay positions itself absolutely inside the host element
 * provided by `IrisPlayer`.
 */

import { useEffect, useRef } from "react";

import type { SubtitleTrack } from "../manifest-client";
import { mountAssOverlay } from "./ass-overlay";
import { mountPgsOverlay } from "./pgs-overlay";

export type SubtitleOverlayKind = "none" | "native" | "ass" | "pgs";

export function subtitleOverlayKind(track: SubtitleTrack | null | undefined): SubtitleOverlayKind {
  if (!track) return "none";
  const codec = track.codec.toLowerCase();
  if (codec === "ass" || codec === "ssa") return "ass";
  if (codec.includes("pgs") || codec.startsWith("hdmv_") || codec.includes("dvb") || codec.includes("dvd_sub")) {
    return "pgs";
  }
  return "native";
}

type Props = {
  /** Container the overlay should render into. `IrisPlayer` provides
   *  a relative-positioned wrapper for this purpose. */
  host: HTMLElement | null;
  track: SubtitleTrack | null;
  /** Master clock in seconds, used when no `<video>` element is
   *  available (Tier C/D). */
  getCurrentTime: () => number;
};

export function SubtitleOverlay({ host, track, getCurrentTime }: Props) {
  // We use a ref to the latest `getCurrentTime` so the effect doesn't
  // re-mount on every parent render.
  const clockRef = useRef(getCurrentTime);
  clockRef.current = getCurrentTime;

  useEffect(() => {
    if (!host || !track) return;
    const kind = subtitleOverlayKind(track);
    if (kind === "none" || kind === "native") return;

    let cancelled = false;
    let handle: { dispose: () => void } | null = null;

    const mountFn = kind === "ass" ? mountAssOverlay : mountPgsOverlay;
    void mountFn({
      host,
      subUrl: track.url,
      getCurrentTime: () => clockRef.current(),
    })
      .then((h) => {
        if (cancelled) h.dispose();
        else handle = h;
      })
      .catch((e) => {
        console.error(`[iris-core] ${kind} overlay failed`, e);
      });

    return () => {
      cancelled = true;
      handle?.dispose();
      handle = null;
    };
  }, [host, track]);

  return null;
}
