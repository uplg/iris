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

import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";

import { IrisChrome, toggleFullscreen } from "./IrisChrome";
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
  /** Saved picks restored from server progress, if any. The parent
   *  passes the values it read from `/progress`; we surface them
   *  back via `onAudioTrackChange` / `onActiveSubtitleChange` so
   *  parent can re-write them on every change. */
  initialAudioIndex?: number;
  initialSubtitleStreamIdx?: number | null;

  onTimeUpdate: (seconds: number) => void;
  onDurationChange: (seconds: number) => void;
  onSeeking: (seconds: number) => void;
  onPause: (seconds: number) => void;
  onEnded: () => void;
  onError: (message: string) => void;
  /** Fires when the user picks a different audio track (index into
   *  `manifest.audio`). Lets `WatchPage` persist the pick. */
  onAudioTrackChange?: (index: number) => void;
  /** Fires when the user picks a different subtitle (by manifest
   *  `stream_idx`). `null` means "no subtitles". */
  onActiveSubtitleChange?: (streamIdx: number | null) => void;
  /** Token that changes when the upstream subtitle source (the torrent
   *  source MKV) has accumulated new bytes since the last extraction
   *  the client received. Threaded into overlay subtitle URLs as a
   *  `?v=<token>` cache-buster: when it bumps, the SubtitleOverlay
   *  hot-reloads via `libass.setTrackByUrl` in place — no worker
   *  recreate, no canvas flash, no re-pick from the menu. Drives the
   *  "subs catch up as the torrent downloads" UX. The token should
   *  flip to a stable value (e.g. `"final"`) once the torrent is
   *  finished so the URL stops mutating and the response can be HTTP
   *  cached. */
  subtitleVersion?: string;
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
  // Mirror IrisChrome's controls visibility so we can hide the
  // mouse cursor over the entire player surface while playback is
  // uninterrupted — matches native fullscreen players. Starts
  // visible so the user always sees the cursor on initial mount;
  // the chrome flips this to `false` after 2.5 s of mouse-still.
  const [controlsVisible, setControlsVisible] = useState(true);
  // Stable DOM node the engine mounts its `<video>` (or `<canvas>`)
  // into. We create it ONCE, outside React's reconciliation, so that
  // when `createPortal` moves the player tree into a Document
  // Picture-in-Picture window, the engine's media element doesn't
  // get orphaned in the original document. Instead, our callback
  // ref (`mountSlotRef` below) re-appends this stable host into
  // whichever React-managed slot is currently mounted, in whichever
  // document. Cross-document `appendChild` performs WHATWG-spec
  // "adopting steps" automatically; modern Chromium/Firefox keep
  // `<video>` playback alive across the move.
  const videoHostRef = useRef<HTMLDivElement | null>(null);
  if (videoHostRef.current === null && typeof document !== "undefined") {
    const host = document.createElement("div");
    host.className = "absolute inset-0";
    videoHostRef.current = host;
  }
  const mountSlotRef = useCallback((slot: HTMLDivElement | null) => {
    const host = videoHostRef.current;
    if (!host || !slot) return;
    if (host.parentElement !== slot) {
      slot.appendChild(host);
    }
  }, []);
  const handleRef = useRef<EngineHandle | null>(null);
  const [handle, setHandle] = useState<EngineHandle | null>(null);
  const currentTimeRef = useRef<number>(props.startPosition);
  // Latched "were we playing right before the last engine teardown?"
  // The mount-effect cleanup snapshots `!handle.paused()` here so the
  // next mount can auto-resume — without this an audio-track switch
  // (Tier B/C/E only — those tiers remount on pick) leaves the user
  // staring at a paused frame.
  const playingBeforeRemountRef = useRef<boolean>(false);
  const pip = useDocumentPip({ width: 720, height: 405 });

  // Active audio track index into `manifest.audio`. Initial pick:
  // 1. `props.initialAudioIndex` if the parent restored one from
  //    server progress (and the index is still valid for this
  //    manifest — track count can change on a re-ingest).
  // 2. The file's `default` track (first one flagged) if any.
  // 3. Otherwise 0.
  const defaultAudioIndex = useMemo(() => {
    const flagged = props.manifest.audio.findIndex((a) => a.default);
    return flagged >= 0 ? flagged : 0;
  }, [props.manifest]);
  const [audioTrackIndex, setAudioTrackIndex] = useState<number>(() => {
    const restored = props.initialAudioIndex;
    if (
      typeof restored === "number" &&
      restored >= 0 &&
      restored < props.manifest.audio.length
    ) {
      return restored;
    }
    return defaultAudioIndex;
  });
  // Position passed to the engine on (re)mount. Starts at the
  // resume offset; on an audio-track switch that needs a remount
  // (tiers A/B/C/E) we bump it to the current playhead so the
  // user lands back where they were, not at startPosition.
  const [mountStartPosition, setMountStartPosition] = useState<number>(
    props.startPosition,
  );
  // Remount-trigger counter. We can't put `audioTrackIndex` itself
  // in the mount effect's deps: that fires a remount for Tier F too,
  // which would wipe out the live `hls.audioTrack` switch and snap
  // back to hls.js's default audio. Instead we bump this version
  // ONLY for tiers that need a remount (A/B/C/E), and pass the
  // current `audioTrackIndex` to the engine through closure.
  const [audioRemountVersion, bumpAudioRemount] = useReducer(
    (x: number) => x + 1,
    0,
  );

  const { native: nativeSubs, overlay: overlaySubs } = useMemo(
    () => classifySubtitles(props.manifest),
    [props.manifest],
  );

  // Unified subtitle state: any subtitle, native or overlay. The
  // chrome's menu writes here. Initial pick:
  //   1. `props.initialSubtitleStreamIdx` if the parent restored
  //      one from server progress — `null` means "user explicitly
  //      turned subs off", `undefined` means "no saved pick".
  //   2. The file's `default` track if any.
  //   3. Otherwise the first native (WebVTT-renderable) track if any.
  //   4. Otherwise the first overlay track (ASS/PGS) — for BluRay
  //      remuxes that only ship PGS, this is what the user wants.
  const [activeSubtitle, setActiveSubtitle] = useState<SubtitleTrack | null>(() => {
    const restored = props.initialSubtitleStreamIdx;
    if (restored === null) return null;
    if (typeof restored === "number") {
      const match = props.manifest.subtitles.find((s) => s.stream_idx === restored);
      if (match) return match;
    }
    const def = props.manifest.subtitles.find((s) => s.default);
    if (def) return def;
    const nativeFirst = props.manifest.subtitles.find(
      (s) => subtitleOverlayKind(s) === "native",
    );
    if (nativeFirst) return nativeFirst;
    return props.manifest.subtitles[0] ?? null;
  });

  // Wrapper around `setActiveSubtitle` that ALSO fires the
  // `onActiveSubtitleChange` callback so the parent can persist the
  // pick. Used by the chrome menu (passed as `onSubtitleChange`).
  // We don't fire from the bare `setActiveSubtitle` because it's
  // also called internally on initial mount with no user intent.
  const onSubtitlePick = useCallback(
    (track: SubtitleTrack | null) => {
      setActiveSubtitle(track);
      props.onActiveSubtitleChange?.(track ? track.stream_idx : null);
    },
    [props.onActiveSubtitleChange],
  );

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
  // Uses the stable `videoHostRef` so PiP / portal moves don't
  // trigger a remount — see the comment on `videoHostRef`.
  useEffect(() => {
    const container = videoHostRef.current;
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
          startPosition: mountStartPosition,
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
          // Auto-resume if the previous engine was playing right
          // before this remount fired (typically an audio-track
          // switch on Tier B/C/E). The first play attempt might
          // happen before the engine is fully primed; the .catch
          // keeps the failure silent so we don't double-fire
          // onError. The flag is consumed (set back to false) so it
          // only triggers for THIS specific remount.
          if (playingBeforeRemountRef.current) {
            playingBeforeRemountRef.current = false;
            void h.play().catch(() => {
              /* engine not ready yet; user can hit play manually */
            });
          }
        }
      } catch (e) {
        if (!cancelled) props.onError(e instanceof Error ? e.message : String(e));
      }
    })();

    return () => {
      cancelled = true;
      const old = handleRef.current;
      if (old) {
        try {
          playingBeforeRemountRef.current = !old.paused();
        } catch {
          playingBeforeRemountRef.current = false;
        }
      }
      handleRef.current = null;
      setHandle(null);
      void old?.dispose();
    };
    // The mount effect re-fires when the *triggering* identity
    // changes — tier, src, resume position, or our explicit
    // `audioRemountVersion` counter. `audioTrackIndex` deliberately
    // isn't a dep: it gets captured by closure and reflects the
    // user's latest pick at the moment the effect actually runs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.tier, props.src, mountStartPosition, audioRemountVersion]);

  const onAudioPick = (id: string) => {
    const idx = Number(id);
    if (!Number.isFinite(idx)) return;
    console.log(
      `[iris-core] onAudioPick id=${id} tier=${props.tier} hasHandle=${!!handle} needsRemount=${tierRequiresRemountForAudio(props.tier)}`,
    );
    setAudioTrackIndex(idx);
    props.onAudioTrackChange?.(idx);
    if (tierRequiresRemountForAudio(props.tier)) {
      // Capture the current playhead BEFORE the remount tears the
      // engine down. The engine's `currentTime()` becomes 0 once
      // disposed, so reading from our forwarded `currentTimeRef`
      // (updated on every `timeupdate`) gives the true position.
      setMountStartPosition(currentTimeRef.current);
      bumpAudioRemount();
    } else if (handle) {
      // Tier F: hls.js switches the rendition live. No remount
      // needed — and crucially we mustn't bump `audioRemountVersion`
      // here, otherwise the mount effect would re-fire and spin up a
      // fresh hls.js instance that snaps back to its default audio.
      handle.setAudioTrack(id);
    } else {
      console.warn(
        "[iris-core] onAudioPick: tier doesn't need remount but handle is null — swallowed",
      );
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
  // The URL is decorated with the current `subtitleVersion` token so the
  // SubtitleOverlay's URL-watch effect can hot-reload through libass /
  // libpgs as the torrent download progresses. When `subtitleVersion`
  // is undefined (parent didn't thread it, e.g. tests / older callers)
  // the URL stays bare and there's no auto-refresh — old behaviour.
  const activeOverlay = useMemo(() => {
    if (activeSubtitle == null) return null;
    if (subtitleOverlayKind(activeSubtitle) === "native") return null;
    const base = overlaySubs.find((s) => s.stream_idx === activeSubtitle.stream_idx);
    if (!base) return null;
    if (!props.subtitleVersion) return base;
    return { ...base, url: `${base.url}?v=${encodeURIComponent(props.subtitleVersion)}` };
  }, [activeSubtitle, overlaySubs, props.subtitleVersion]);

  // Click-to-toggle handling. Single click → play/pause, double
  // click → fullscreen. Since the browser fires `click` for BOTH
  // clicks in a double-click sequence (before `dblclick`), we
  // defer the play/pause via a short timer and cancel it if a
  // second click arrives. 250 ms matches the OS-level double-click
  // threshold on most platforms; longer would feel laggy on single
  // clicks, shorter would miss legit double clicks.
  const clickTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const shouldIgnoreClick = useCallback(
    (e: React.MouseEvent<HTMLDivElement>): boolean => {
      // Ignore clicks that land on (or inside) interactive chrome
      // bits — the chrome's own play button + scrubber + menus
      // already handle themselves; toggling here on top would
      // double-fire (play→pause→play within one click).
      let n: HTMLElement | null = e.target as HTMLElement;
      while (n && n !== e.currentTarget) {
        const tag = n.tagName;
        if (
          tag === "BUTTON" ||
          tag === "INPUT" ||
          tag === "A" ||
          tag === "SELECT" ||
          tag === "TEXTAREA" ||
          n.dataset.irisChrome !== undefined
        ) {
          return true;
        }
        n = n.parentElement;
      }
      return false;
    },
    [],
  );

  const onSurfaceClick = useCallback(
    (e: React.MouseEvent<HTMLDivElement>) => {
      if (shouldIgnoreClick(e)) return;
      const h = handleRef.current;
      if (!h) return;
      if (clickTimerRef.current) clearTimeout(clickTimerRef.current);
      clickTimerRef.current = setTimeout(() => {
        clickTimerRef.current = null;
        if (h.paused()) {
          void h.play().catch(() => undefined);
        } else {
          h.pause();
        }
      }, 250);
    },
    [shouldIgnoreClick],
  );

  const onSurfaceDoubleClick = useCallback(
    (e: React.MouseEvent<HTMLDivElement>) => {
      if (shouldIgnoreClick(e)) return;
      if (clickTimerRef.current) {
        clearTimeout(clickTimerRef.current);
        clickTimerRef.current = null;
      }
      void toggleFullscreen(wrapper);
    },
    [shouldIgnoreClick, wrapper],
  );

  useEffect(() => () => {
    if (clickTimerRef.current) clearTimeout(clickTimerRef.current);
  }, []);

  const playerNode = (
    <div
      ref={setWrapper}
      onClick={onSurfaceClick}
      onDoubleClick={onSurfaceDoubleClick}
      className={
        "relative h-full w-full bg-black" +
        (controlsVisible ? "" : " cursor-none")
      }
    >
      {/* React-managed slot. The callback ref re-appends our stable
          `videoHostRef` into this slot whenever React mounts a new
          one (e.g., after a Document PiP open/close moves the
          player tree across documents). The engine's `<video>`
          lives inside `videoHostRef` and is carried along
          automatically — playback state survives the move. */}
      <div ref={mountSlotRef} className="absolute inset-0" />
      <SubtitleOverlay
        host={wrapper}
        track={activeOverlay}
        getCurrentTime={() => currentTimeRef.current}
      />
      <IrisChrome
        handle={handle}
        manifest={props.manifest}
        activeSubtitle={activeSubtitle}
        onSubtitleChange={onSubtitlePick}
        activeAudioIndex={audioTrackIndex}
        onAudioPick={onAudioPick}
        fullscreenTarget={wrapper}
        onControlsVisibleChange={setControlsVisible}
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
