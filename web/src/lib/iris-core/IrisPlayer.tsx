/**
 * Iris player. Vidstack-free.
 *
 * One imperative branch for every tier — `IrisPlayer` picks the right
 * engine (`mountTierA/B/C/F`), mounts it into a wrapper `<div>`, and
 * layers `IrisChrome` (custom controls) and `SubtitleOverlay` (ASS /
 * PGS rendering) on top. Native `<track>` subtitles stay inside the
 * engine's `<video>` (Tier A/B/F); the chrome's subtitle menu picks
 * which track is active.
 */

import { useEffect, useMemo, useRef, useState } from "react";

import { IrisChrome } from "./IrisChrome";
import type { EngineHandle, EngineMount, NativeSubtitleTrack } from "./engine";
import type { DecodeTier, Manifest, SubtitleTrack } from "./manifest-client";
import { nativeSubtitleUrl } from "./manifest-client";
import { attachMediaSession } from "./os/media-session";
import { mountDocumentPip, type DocumentPipHandle } from "./os/document-pip";
import { SubtitleOverlay, subtitleOverlayKind } from "./subs/subtitle-overlay";
import { mountTierA } from "./tiers/tier-a-native";

/**
 * Dynamic-import map for the heavy engines. Tier A is statically
 * imported because (1) it's tiny and (2) it's the warm path on most
 * sessions. The others pull in Mediabunny / hls.js on demand so the
 * initial chunk only carries the chrome + Tier A logic.
 */
const ENGINE_LOADERS: Record<DecodeTier, () => Promise<{ mount: EngineMount }>> = {
  A: () => Promise.resolve({ mount: mountTierA }),
  B: () => import("./tiers/tier-b-mse").then((m) => ({ mount: m.mountTierB })),
  C: () => import("./tiers/tier-c-webcodecs").then((m) => ({ mount: m.mountTierC })),
  D: () => import("./tiers/tier-c-webcodecs").then((m) => ({ mount: m.mountTierC })),
  E: () => import("./tiers/tier-e-hevcjs").then((m) => ({ mount: m.mountTierE })),
  F: () => import("./tiers/tier-f-hls").then((m) => ({ mount: m.mountTierF })),
};

export type IrisPlayerProps = {
  tier: DecodeTier;
  src: string;
  /** Reserved for future tier-specific MIME handling; ignored today. */
  srcType: string;
  title: string;
  manifest: Manifest;
  startPosition: number;

  onTimeUpdate: (seconds: number) => void;
  onDurationChange: (seconds: number) => void;
  onSeeking: (seconds: number) => void;
  onPause: (seconds: number) => void;
  onEnded: () => void;
  onError: (message: string) => void;
};

function classifySubtitles(manifest: Manifest): {
  native: NativeSubtitleTrack[];
  overlay: SubtitleTrack[];
} {
  const native: NativeSubtitleTrack[] = [];
  const overlay: SubtitleTrack[] = [];
  for (const t of manifest.subtitles) {
    const kind = subtitleOverlayKind(t);
    if (kind === "ass" || kind === "pgs") overlay.push(t);
    else if (kind === "native") native.push({ ...t, vttUrl: nativeSubtitleUrl(manifest, t.stream_idx) });
  }
  return { native, overlay };
}

export function IrisPlayer(props: IrisPlayerProps) {
  const wrapperRef = useRef<HTMLDivElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const handleRef = useRef<EngineHandle | null>(null);
  const [handle, setHandle] = useState<EngineHandle | null>(null);
  const currentTimeRef = useRef<number>(props.startPosition);
  const pipRef = useRef<DocumentPipHandle | null>(null);

  const { native: nativeSubs, overlay: overlaySubs } = useMemo(
    () => classifySubtitles(props.manifest),
    [props.manifest],
  );

  // Unified subtitle state: any subtitle, native or overlay. The
  // chrome's menu writes here.
  const [activeSubtitle, setActiveSubtitle] = useState<SubtitleTrack | null>(() => {
    const def = props.manifest.subtitles.find((s) => s.default) ?? null;
    return def;
  });

  // For native subtitles, we toggle the `<track>` element's `mode`
  // attribute when the user picks one. The chrome's selection drives
  // this. Tier C has no <track>, so it's a no-op there.
  useEffect(() => {
    const video = handle?.videoElement?.();
    if (!video) return;
    const tracks = video.textTracks;
    if (!tracks) return;
    for (let i = 0; i < tracks.length; i += 1) {
      const t = tracks[i];
      const sub = nativeSubs[i];
      if (!t || !sub) continue;
      const isActive =
        activeSubtitle != null &&
        subtitleOverlayKind(activeSubtitle) === "native" &&
        activeSubtitle.stream_idx === sub.stream_idx;
      t.mode = isActive ? "showing" : "disabled";
    }
  }, [handle, activeSubtitle, nativeSubs]);

  // Mount the engine on `(tier, src, startPosition)` change.
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const loader = ENGINE_LOADERS[props.tier];
    if (!loader) {
      props.onError(`No engine wired for tier ${props.tier}`);
      return;
    }
    let cancelled = false;

    void (async () => {
      try {
        const { mount } = await loader();
        if (cancelled) return;
        const h = await mount({
          container,
          manifest: props.manifest,
          streamUrl: props.src,
          startPosition: props.startPosition,
          nativeSubs,
          onTimeUpdate: (t) => {
            currentTimeRef.current = t;
            props.onTimeUpdate(t);
          },
          onDurationChange: props.onDurationChange,
          onSeeking: props.onSeeking,
          onPause: props.onPause,
          onEnded: props.onEnded,
          onError: (err) => props.onError(err.message),
        });
        if (cancelled) {
          void h.dispose();
        } else {
          handleRef.current = h;
          setHandle(h);
        }
      } catch (e) {
        if (!cancelled) props.onError(e instanceof Error ? e.message : String(e));
      }
    })();

    return () => {
      cancelled = true;
      const old = handleRef.current;
      handleRef.current = null;
      setHandle(null);
      void old?.dispose();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.tier, props.src, props.startPosition]);

  // Media Session wiring — runs alongside any engine, so OS media keys
  // and lock-screen art work even for canvas-only Tier C/D.
  useEffect(() => {
    if (!handle) return;
    const wire = attachMediaSession(handle, props.manifest, {
      title: props.title,
    });
    return wire.dispose;
  }, [handle, props.manifest, props.title]);

  // Document PiP controller — `IrisChrome` reads `pipRef.current` to
  // render the toggle button. Mounted once per wrapper lifetime.
  useEffect(() => {
    const wrapper = wrapperRef.current;
    if (!wrapper) return;
    pipRef.current = mountDocumentPip(wrapper);
    return () => {
      pipRef.current = null;
    };
  }, []);

  // Overlay subtitle: only when the active sub needs overlay rendering.
  const activeOverlay =
    activeSubtitle != null && subtitleOverlayKind(activeSubtitle) !== "native"
      ? overlaySubs.find((s) => s.stream_idx === activeSubtitle.stream_idx) ?? null
      : null;

  return (
    <div ref={wrapperRef} className="relative h-full w-full bg-black">
      {/* Engine mount point. The engine inserts a <video> or <canvas>
          here; the chrome and overlay are positioned on top. */}
      <div ref={containerRef} className="absolute inset-0" />
      <SubtitleOverlay
        host={wrapperRef.current}
        track={activeOverlay}
        getCurrentTime={() => currentTimeRef.current}
      />
      <IrisChrome
        handle={handle}
        manifest={props.manifest}
        activeSubtitle={activeSubtitle}
        onSubtitleChange={setActiveSubtitle}
        fullscreenTarget={wrapperRef.current}
        documentPip={pipRef.current}
        title={props.title}
      />
    </div>
  );
}
