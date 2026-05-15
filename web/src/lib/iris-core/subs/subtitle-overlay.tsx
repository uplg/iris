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
import { mountAssOverlay, type AssOverlayHandle } from "./ass-overlay";
import { mountPgsOverlay, type PgsOverlayHandle } from "./pgs-overlay";

type OverlayHandle = AssOverlayHandle | PgsOverlayHandle;

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

  // Two-effect split:
  //   1) Mount/unmount on track *identity* (stream_idx + codec). This
  //      is the only path that destroys + re-creates the libass worker.
  //   2) Call `handle.setUrl(track.url)` on URL *content* changes —
  //      typically the `?v=<progress>` cache-buster bumping as the
  //      torrent download advances. This re-fetches the `.ass` in-place
  //      without a canvas blank-flash and without the user having to
  //      re-pick the track from the menu.
  const handleRef = useRef<OverlayHandle | null>(null);
  // Live mirror of `track.url` — the mount-effect's `.then` reads this
  // after the (async) worker init resolves to reconcile against any URL
  // drift that happened in the meantime (e.g., the torrent crossed a
  // progress milestone and the parent re-rendered with `?v=` bumped).
  const latestUrlRef = useRef<string | null>(track?.url ?? null);
  latestUrlRef.current = track?.url ?? null;
  const streamIdx = track?.stream_idx ?? null;
  const codec = track?.codec ?? null;
  const kind = subtitleOverlayKind(track);

  useEffect(() => {
    if (!host || !track) return;
    if (kind === "none" || kind === "native") return;

    let cancelled = false;
    const initialUrl = track.url;
    const mountFn = kind === "ass" ? mountAssOverlay : mountPgsOverlay;
    void mountFn({
      host,
      subUrl: initialUrl,
      getCurrentTime: () => clockRef.current(),
    })
      .then((h) => {
        if (cancelled) {
          h.dispose();
          return;
        }
        handleRef.current = h;
        // Reconcile if the URL bumped while the worker was spinning up.
        if (latestUrlRef.current && latestUrlRef.current !== initialUrl) {
          h.setUrl(latestUrlRef.current);
        }
      })
      .catch((e) => {
        console.error(`[iris-core] ${kind} overlay failed`, e);
      });

    return () => {
      cancelled = true;
      handleRef.current?.dispose();
      handleRef.current = null;
    };
    // Re-mount only when the track identity changes — not on URL-only
    // updates, which the second effect handles in-place.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [host, streamIdx, codec, kind]);

  useEffect(() => {
    if (!track) return;
    const h = handleRef.current;
    if (!h) return;
    h.setUrl(track.url);
  }, [track?.url, track]);

  return null;
}
