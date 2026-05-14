/**
 * Custom player chrome — replaces Vidstack's default layout. Receives
 * an `EngineHandle` from `IrisPlayer` and reads/writes its state to
 * drive a scrub bar + play/pause + volume + fullscreen + unified
 * subtitle/audio pickers.
 *
 * Design rules:
 * - Engine-agnostic: no `<video>`-specific paths here. Tier C/D
 *   (canvas) and Tier A/B/F (`<video>`) flow through the same code.
 * - Polls the handle every animation frame for `currentTime` and
 *   `paused` — engines that don't emit fine-grained events still get
 *   accurate UI updates.
 * - Subtitle picker is *unified*: WebVTT (native `<track>`), ASS
 *   (libass overlay), PGS (libpgs overlay) all appear in one list
 *   labelled with their format.
 */

import { useEffect, useRef, useState } from "react";
import {
  Captions,
  Languages,
  Maximize,
  Pause,
  PictureInPicture2,
  Play,
  Rewind,
  FastForward,
  Volume2,
  VolumeX,
} from "lucide-react";

import type { EngineAudioTrack, EngineHandle } from "./engine";
import type { Manifest, SubtitleTrack } from "./manifest-client";
import { subtitleOverlayKind } from "./subs/subtitle-overlay";

export type ChromePipControl = {
  supported: boolean;
  isActive: boolean;
  toggle: () => Promise<void>;
};

export type IrisChromeProps = {
  handle: EngineHandle | null;
  manifest: Manifest;
  /** Active subtitle track (for both native and overlay paths). null = off. */
  activeSubtitle: SubtitleTrack | null;
  onSubtitleChange: (track: SubtitleTrack | null) => void;
  /** Index into `manifest.audio` of the currently-active track. The
   *  chrome highlights it; user picks call `onAudioPick`. */
  activeAudioIndex: number;
  /** Called when the user picks an audio track from the menu. Bridges
   *  to either `handle.setAudioTrack` (Tier F) or a remount (others). */
  onAudioPick: (id: string) => void;
  /** Container element used as the fullscreen target. */
  fullscreenTarget: HTMLElement | null;
  /** Document PiP controller. `supported=false` hides the button. */
  documentPip: ChromePipControl;
  title: string;
};

export function IrisChrome(props: IrisChromeProps) {
  const { handle, manifest } = props;

  // Polled state. We deliberately don't trust engine events for the
  // chrome's display state because engines emit at different cadences;
  // a 60-Hz tick gives us a consistent UI feel.
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState<number | null>(manifest.duration_s);
  const [paused, setPaused] = useState(true);
  const [volume, setVolume] = useState(1);
  const [muted, setMuted] = useState(false);
  const [buffered, setBuffered] = useState<Array<[number, number]>>([]);
  // Audio tracks come from the manifest (source of truth) — that way
  // even engines whose track-reporting is racey (e.g., hls.js taking
  // a beat to populate `hls.audioTracks` after MANIFEST_PARSED) still
  // show every rendition the user can pick. The chrome marks the
  // active one based on the parent's `activeAudioIndex` state.
  const audioTracks: EngineAudioTrack[] = manifest.audio.map((a, i) => ({
    id: String(i),
    label: a.title ?? a.lang?.toUpperCase() ?? `Audio ${i + 1}`,
    lang: a.lang ?? undefined,
    active: i === props.activeAudioIndex,
  }));
  const [menu, setMenu] = useState<"none" | "subs" | "audio">("none");
  const [scrubbing, setScrubbing] = useState(false);
  const [hovered, setHovered] = useState(true);
  const scrubTargetRef = useRef<number | null>(null);

  // Auto-hide controls after 2.5s of mouse-still during playback.
  const hideTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const resetHideTimer = () => {
    setHovered(true);
    if (hideTimer.current) clearTimeout(hideTimer.current);
    hideTimer.current = setTimeout(() => {
      if (!paused && !scrubbing && menu === "none") setHovered(false);
    }, 2500);
  };

  // rAF loop drives the displayed state from the engine.
  useEffect(() => {
    if (!handle) return;
    let rafId = 0;
    const tick = () => {
      if (!scrubbing) setCurrentTime(handle.currentTime());
      setDuration(handle.duration());
      setPaused(handle.paused());
      setVolume(handle.volume());
      setMuted(handle.muted());
      setBuffered(handle.buffered());
      rafId = requestAnimationFrame(tick);
    };
    rafId = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(rafId);
  }, [handle, scrubbing]);

  // Keyboard shortcuts on the fullscreen container.
  useEffect(() => {
    if (!handle || !props.fullscreenTarget) return;
    const el = props.fullscreenTarget;
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement) return;
      switch (e.key) {
        case " ":
        case "k":
          e.preventDefault();
          if (handle.paused()) void handle.play();
          else handle.pause();
          break;
        case "ArrowLeft":
        case "j":
          e.preventDefault();
          handle.seek(Math.max(0, handle.currentTime() - 10));
          break;
        case "ArrowRight":
        case "l":
          e.preventDefault();
          handle.seek(handle.currentTime() + 10);
          break;
        case "f":
          e.preventDefault();
          void toggleFullscreen(props.fullscreenTarget);
          break;
        case "m":
          e.preventDefault();
          handle.setMuted(!handle.muted());
          break;
        case "ArrowUp":
          e.preventDefault();
          handle.setVolume(Math.min(1, handle.volume() + 0.1));
          break;
        case "ArrowDown":
          e.preventDefault();
          handle.setVolume(Math.max(0, handle.volume() - 0.1));
          break;
        default:
          break;
      }
    };
    // Make the container focusable so it actually receives key events.
    if (el.tabIndex < 0) el.tabIndex = 0;
    el.addEventListener("keydown", onKey);
    return () => el.removeEventListener("keydown", onKey);
  }, [handle, props.fullscreenTarget]);

  // Click-to-toggle is handled at the wrapper level in `IrisPlayer`
  // — see `onSurfaceClick` there. We used to also register a
  // `flex-1.onClick={...}` here that was supposed to ignore chrome-
  // internal clicks, but the `closest('[data-iris-chrome]')` check
  // matched our own root wrapper (which carries `data-iris-chrome`)
  // and consequently bailed on every click, so it was always dead
  // code. Removed; we let pointer events pass through the
  // transparent surface area to the wrapper instead.

  if (!handle) return null;

  return (
    <div
      data-iris-chrome
      onMouseMove={resetHideTimer}
      onMouseLeave={() => {
        if (hideTimer.current) clearTimeout(hideTimer.current);
        if (!paused) setHovered(false);
      }}
      className="absolute inset-0 z-10 flex flex-col"
    >
      {/* Transparent surface above the engine. We want clicks here to
          fall through to `IrisPlayer`'s wrapper-level `onSurfaceClick`
          (which toggles play/pause), so we deliberately disable
          pointer events on the surface itself — interactive children
          (chrome bar / scrubber / menus) re-enable them via their own
          `pointer-events-auto`. */}
      <div className="pointer-events-none flex-1" />

      <div
        className={`pointer-events-auto flex flex-col gap-1 bg-gradient-to-t from-black/80 to-transparent px-3 pb-2 pt-12 text-white transition-opacity duration-200 ${
          hovered || paused || menu !== "none" ? "opacity-100" : "opacity-0"
        }`}
      >
        {/* Title sits above the controls bar but only while hovered. */}
        <div className="line-clamp-1 text-xs opacity-80">{props.title}</div>

        {/* Scrub bar */}
        <ScrubBar
          durationSeconds={duration}
          currentSeconds={scrubbing ? scrubTargetRef.current ?? currentTime : currentTime}
          bufferedRanges={buffered}
          onScrub={(s) => {
            setScrubbing(true);
            scrubTargetRef.current = s;
            setCurrentTime(s);
          }}
          onScrubEnd={(s) => {
            handle.seek(s);
            scrubTargetRef.current = null;
            setScrubbing(false);
          }}
        />

        <div className="flex items-center gap-1 text-[12px]">
          <Button
            label={paused ? "Play" : "Pause"}
            icon={paused ? <Play className="size-5" /> : <Pause className="size-5" />}
            onClick={() => (paused ? void handle.play() : handle.pause())}
          />
          <Button
            label="-10s"
            icon={<Rewind className="size-4" />}
            onClick={() => handle.seek(Math.max(0, handle.currentTime() - 10))}
          />
          <Button
            label="+10s"
            icon={<FastForward className="size-4" />}
            onClick={() => handle.seek(handle.currentTime() + 10)}
          />
          <TimeDisplay current={currentTime} total={duration} />

          <div className="flex-1" />

          <VolumeControl
            volume={volume}
            muted={muted}
            onMute={() => handle.setMuted(!muted)}
            onVolume={(v) => handle.setVolume(v)}
          />

          {/* Subtitle picker — unified list (WebVTT + ASS + PGS). */}
          {manifest.subtitles.length > 0 && (
            <MenuButton
              label={`Subs${
                props.activeSubtitle
                  ? ` · ${props.activeSubtitle.lang?.toUpperCase() ?? "ON"}`
                  : ""
              }`}
              icon={<Captions className="size-4" />}
              open={menu === "subs"}
              onToggle={() => setMenu(menu === "subs" ? "none" : "subs")}
            >
              <SubtitleMenu
                subs={manifest.subtitles}
                active={props.activeSubtitle}
                onSelect={(t) => {
                  props.onSubtitleChange(t);
                  setMenu("none");
                }}
              />
            </MenuButton>
          )}

          {/* Audio picker — multi-language / multi-channel. */}
          {audioTracks.length > 1 && (
            <MenuButton
              label="Audio"
              icon={<Languages className="size-4" />}
              open={menu === "audio"}
              onToggle={() => setMenu(menu === "audio" ? "none" : "audio")}
            >
              <AudioMenu
                tracks={audioTracks}
                onSelect={(id) => {
                  props.onAudioPick(id);
                  setMenu("none");
                }}
              />
            </MenuButton>
          )}

          {props.documentPip.supported && (
            <Button
              label="Picture-in-picture"
              icon={<PictureInPicture2 className="size-4" />}
              onClick={() => void props.documentPip.toggle()}
              active={props.documentPip.isActive}
            />
          )}
          <Button
            label="Fullscreen"
            icon={<Maximize className="size-4" />}
            onClick={() => void toggleFullscreen(props.fullscreenTarget)}
          />
        </div>
      </div>
    </div>
  );
}

function ScrubBar(props: {
  durationSeconds: number | null;
  currentSeconds: number;
  bufferedRanges: Array<[number, number]>;
  onScrub: (seconds: number) => void;
  onScrubEnd: (seconds: number) => void;
}) {
  const trackRef = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);
  if (!props.durationSeconds || props.durationSeconds <= 0) {
    return (
      <div className="h-1.5 w-full rounded bg-white/15" aria-hidden>
        {/* No duration yet — show a flat track */}
      </div>
    );
  }
  const dur = props.durationSeconds;
  const pct = Math.min(100, Math.max(0, (props.currentSeconds / dur) * 100));

  const seekFromEvent = (e: PointerEvent | React.PointerEvent): number => {
    const track = trackRef.current;
    if (!track) return 0;
    const rect = track.getBoundingClientRect();
    const x = Math.max(0, Math.min(rect.width, e.clientX - rect.left));
    return (x / rect.width) * dur;
  };
  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    e.currentTarget.setPointerCapture(e.pointerId);
    dragging.current = true;
    props.onScrub(seekFromEvent(e));
  };
  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!dragging.current) return;
    props.onScrub(seekFromEvent(e));
  };
  const onPointerUp = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!dragging.current) return;
    dragging.current = false;
    const target = seekFromEvent(e);
    props.onScrubEnd(target);
  };

  return (
    <div
      ref={trackRef}
      className="relative h-2 w-full cursor-pointer rounded bg-white/15"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      role="slider"
      aria-valuemin={0}
      aria-valuemax={dur}
      aria-valuenow={props.currentSeconds}
    >
      {/* Buffered ranges */}
      {props.bufferedRanges.map(([s, e], i) => {
        const left = (s / dur) * 100;
        const width = ((e - s) / dur) * 100;
        return (
          <div
            key={i}
            className="absolute top-0 h-full bg-white/30"
            style={{ left: `${left}%`, width: `${width}%` }}
          />
        );
      })}
      {/* Playhead progress */}
      <div className="absolute left-0 top-0 h-full rounded bg-primary" style={{ width: `${pct}%` }} />
      {/* Knob */}
      <div
        className="absolute top-1/2 h-3.5 w-3.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-white shadow"
        style={{ left: `${pct}%` }}
      />
    </div>
  );
}

function TimeDisplay({ current, total }: { current: number; total: number | null }) {
  return (
    <span className="ml-1 select-none font-mono text-[12px] tabular-nums opacity-80">
      {formatTime(current)} / {total != null ? formatTime(total) : "--:--"}
    </span>
  );
}

function VolumeControl(props: {
  volume: number;
  muted: boolean;
  onMute: () => void;
  onVolume: (v: number) => void;
}) {
  const display = props.muted ? 0 : props.volume;
  return (
    <div className="group flex items-center gap-1">
      <Button
        label={props.muted ? "Unmute" : "Mute"}
        icon={
          props.muted ? <VolumeX className="size-4" /> : <Volume2 className="size-4" />
        }
        onClick={props.onMute}
      />
      <input
        type="range"
        min={0}
        max={1}
        step={0.01}
        value={display}
        onChange={(e) => props.onVolume(Number(e.target.value))}
        className="h-1 w-0 cursor-pointer appearance-none rounded bg-white/30 transition-all group-hover:w-20"
      />
    </div>
  );
}

function MenuButton(props: {
  label: string;
  icon: React.ReactNode;
  open: boolean;
  onToggle: () => void;
  children: React.ReactNode;
}) {
  return (
    <div className="relative">
      <Button label={props.label} icon={props.icon} onClick={props.onToggle} active={props.open} />
      {props.open && (
        <div className="absolute bottom-full right-0 mb-2 max-h-72 w-64 overflow-auto rounded bg-black/90 p-1 shadow-lg ring-1 ring-white/10">
          {props.children}
        </div>
      )}
    </div>
  );
}

function SubtitleMenu(props: {
  subs: SubtitleTrack[];
  active: SubtitleTrack | null;
  onSelect: (track: SubtitleTrack | null) => void;
}) {
  return (
    <ul className="divide-y divide-white/5">
      <li>
        <button
          onClick={() => props.onSelect(null)}
          className={`w-full px-2 py-1.5 text-left text-[12px] hover:bg-white/10 ${
            props.active == null ? "bg-white/10" : ""
          }`}
        >
          Off
        </button>
      </li>
      {props.subs.map((sub) => {
        const kind = subtitleOverlayKind(sub);
        const isActive = props.active?.stream_idx === sub.stream_idx;
        const label = sub.title ?? sub.lang?.toUpperCase() ?? `Sub ${sub.stream_idx}`;
        return (
          <li key={sub.stream_idx}>
            <button
              onClick={() => props.onSelect(sub)}
              className={`flex w-full items-center justify-between gap-2 px-2 py-1.5 text-left text-[12px] hover:bg-white/10 ${
                isActive ? "bg-white/10" : ""
              }`}
            >
              <span className="line-clamp-1">{label}</span>
              <span className="rounded bg-white/10 px-1 py-0.5 text-[9px] uppercase tracking-wide opacity-70">
                {kind === "native" ? sub.codec : kind}
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}

function AudioMenu(props: { tracks: EngineAudioTrack[]; onSelect: (id: string) => void }) {
  return (
    <ul className="divide-y divide-white/5">
      {props.tracks.map((t) => (
        <li key={t.id}>
          <button
            onClick={() => props.onSelect(t.id)}
            className={`flex w-full items-center justify-between gap-2 px-2 py-1.5 text-left text-[12px] hover:bg-white/10 ${
              t.active ? "bg-white/10" : ""
            }`}
          >
            <span className="line-clamp-1">{t.label}</span>
            {t.lang && (
              <span className="rounded bg-white/10 px-1 py-0.5 text-[9px] uppercase tracking-wide opacity-70">
                {t.lang}
              </span>
            )}
          </button>
        </li>
      ))}
    </ul>
  );
}

function Button(props: {
  label: string;
  icon: React.ReactNode;
  onClick: () => void;
  active?: boolean;
}) {
  return (
    <button
      title={props.label}
      aria-label={props.label}
      onClick={props.onClick}
      className={`grid size-9 place-items-center rounded transition-colors hover:bg-white/15 ${
        props.active ? "bg-white/15" : ""
      }`}
    >
      {props.icon}
    </button>
  );
}

function formatTime(sec: number): string {
  if (!Number.isFinite(sec) || sec < 0) return "--:--";
  const total = Math.floor(sec);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

async function toggleFullscreen(target: HTMLElement | null): Promise<void> {
  if (!target) return;
  if (document.fullscreenElement === target) {
    try {
      await document.exitFullscreen();
    } catch {
      /* ignore */
    }
  } else {
    try {
      await target.requestFullscreen();
    } catch {
      /* ignore */
    }
  }
}
