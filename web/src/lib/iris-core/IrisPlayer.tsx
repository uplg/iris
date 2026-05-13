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
import { useDocumentPip } from "./os/document-pip";
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

/** Audio-track switching strategy. Tier F changes audio live via
 *  hls.js. Every other tier requires a re-mount to pick a new
 *  audio source (Tier A is single-audio; B/C have to re-spin the
 *  decode/demux pipeline). */
function tierRequiresRemountForAudio(tier: DecodeTier): boolean {
  return tier !== "F";
}

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
  // Use state (not ref) for the wrapper so children re-render when
  // it's attached. Plain refs don't trigger re-renders, which leaves
  // `<SubtitleOverlay host={null}>` stuck on first render and the
  // overlay never mounts. `useState` + a setter-ref fixes it cleanly.
  const [wrapper, setWrapper] = useState<HTMLDivElement | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const handleRef = useRef<EngineHandle | null>(null);
  const [handle, setHandle] = useState<EngineHandle | null>(null);
  const currentTimeRef = useRef<number>(props.startPosition);
  const pip = useDocumentPip({ width: 720, height: 405 });

  // Active audio track index into `manifest.audio`. Initial pick:
  // the file's `default` track (first one flagged) if any, else 0.
  const initialAudioIndex = useMemo(() => {
    const flagged = props.manifest.audio.findIndex((a) => a.default);
    return flagged >= 0 ? flagged : 0;
  }, [props.manifest]);
  const [audioTrackIndex, setAudioTrackIndex] = useState<number>(initialAudioIndex);

  const { native: nativeSubs, overlay: overlaySubs } = useMemo(
    () => classifySubtitles(props.manifest),
    [props.manifest],
  );

  // Unified subtitle state: any subtitle, native or overlay. The
  // chrome's menu writes here. Initial pick:
  //   1. The file's `default` track if any.
  //   2. Otherwise the first native (WebVTT-renderable) track if any.
  //   3. Otherwise the first overlay track (ASS/PGS) — for BluRay
  //      remuxes that only ship PGS, this is what the user wants.
  const [activeSubtitle, setActiveSubtitle] = useState<SubtitleTrack | null>(() => {
    const def = props.manifest.subtitles.find((s) => s.default);
    if (def) return def;
    const nativeFirst = props.manifest.subtitles.find(
      (s) => subtitleOverlayKind(s) === "native",
    );
    if (nativeFirst) return nativeFirst;
    return props.manifest.subtitles[0] ?? null;
  });

  // Push the active native subtitle into the engine. Engines that
  // back a `<video>` element (A/B/F) flip `<track>.mode` for the
  // matching `stream_idx`; Tier C/D are a no-op until canvas-side
  // WebVTT rendering lands. Re-runs whenever the engine remounts so
  // a freshly-mounted Tier F (after a HEVC demote, say) re-applies
  // the active sub.
  useEffect(() => {
    if (!handle) return;
    const kind = activeSubtitle ? subtitleOverlayKind(activeSubtitle) : "none";
    const idx = kind === "native" ? activeSubtitle?.stream_idx ?? null : null;
    handle.setNativeSubtitle(idx);
    if (activeSubtitle) {
      console.log(
        `[iris-core] active subtitle: stream=${activeSubtitle.stream_idx} kind=${kind} ` +
          `codec=${activeSubtitle.codec} url=${activeSubtitle.url}`,
      );
    }
  }, [handle, activeSubtitle]);

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
          audioTrackIndex,
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
    // `audioTrackIndex` is in the dep list so that tiers without a
    // live audio-switch API (A/B/C/E) remount when the user picks a
    // different audio. Tier F bypasses this — see `onAudioPick` below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.tier, props.src, props.startPosition, audioTrackIndex]);

  const onAudioPick = (id: string) => {
    const idx = Number(id);
    if (!Number.isFinite(idx)) return;
    setAudioTrackIndex(idx);
    if (handle && !tierRequiresRemountForAudio(props.tier)) {
      // Tier F: hls.js switches live; we still update local state so
      // the chrome's "active" highlight matches the click immediately.
      handle.setAudioTrack(id);
    }
  };

  // Media Session wiring — runs alongside any engine, so OS media keys
  // and lock-screen art work even for canvas-only Tier C/D.
  useEffect(() => {
    if (!handle) return;
    const wire = attachMediaSession(handle, props.manifest, {
      title: props.title,
    });
    return wire.dispose;
  }, [handle, props.manifest, props.title]);


  // Overlay subtitle: only when the active sub needs overlay rendering.
  const activeOverlay =
    activeSubtitle != null && subtitleOverlayKind(activeSubtitle) !== "native"
      ? overlaySubs.find((s) => s.stream_idx === activeSubtitle.stream_idx) ?? null
      : null;

  const playerNode = (
    <div ref={setWrapper} className="relative h-full w-full bg-black">
      {/* Engine mount point. The engine inserts a <video> or <canvas>
          here; the chrome and overlay are positioned on top. */}
      <div ref={containerRef} className="absolute inset-0" />
      <SubtitleOverlay
        host={wrapper}
        track={activeOverlay}
        getCurrentTime={() => currentTimeRef.current}
      />
      <IrisChrome
        handle={handle}
        manifest={props.manifest}
        activeSubtitle={activeSubtitle}
        onSubtitleChange={setActiveSubtitle}
        activeAudioIndex={audioTrackIndex}
        onAudioPick={onAudioPick}
        fullscreenTarget={wrapper}
        documentPip={{
          supported:
            typeof window !== "undefined" && "documentPictureInPicture" in window,
          isActive: pip.isActive,
          toggle: pip.toggle,
        }}
        title={props.title}
      />
    </div>
  );

  return <>{pip.renderInto(playerNode)}</>;
}
