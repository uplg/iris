/**
 * Tier F — server-side HLS remux played via `hls.js`.
 *
 * The legacy fallback: ffmpeg + shaka-packager build a CMAF HLS
 * cache; `hls.js` handles the manifest parsing, segment loading,
 * variant selection and multi-audio rendition switching. We use
 * `hls.js` directly (no Vidstack wrapper) so we keep full control of
 * its config + see exactly what it does.
 */

import Hls from "hls.js";

import {
  appendNativeTrack,
  bindVideoCallbacks,
  videoBackedHandle,
  type EngineAudioTrack,
  type EngineHandle,
  type EngineMount,
} from "../engine";

export const mountTierF: EngineMount = async (opts) => {
  const { container, streamUrl, nativeSubs, audioTrackIndex } = opts;
  container.innerHTML = "";
  const video = document.createElement("video");
  video.className = "h-full w-full object-contain";
  video.playsInline = true;
  video.preload = "auto";
  const nativeTrackMap = new Map<number, HTMLTrackElement>();
  for (const sub of nativeSubs) {
    appendNativeTrack(video, sub, nativeTrackMap);
  }
  container.appendChild(video);

  const initialSeek = { done: false };
  const unbind = bindVideoCallbacks(video, opts, initialSeek);
  const onErr = () => {
    const err = video.error;
    opts.onError(new Error(err ? `media error ${err.code}: ${err.message}` : "video element error"));
  };
  video.addEventListener("error", onErr);

  // Decide between hls.js and the browser's native HLS pipeline.
  // Strategy: **prefer hls.js whenever it works**, and reserve the
  // native path for environments where hls.js can't run at all
  // (iOS Safari, which has no MSE → `Hls.isSupported()` is false).
  //
  // Past versions of this file flipped the priority — "use native
  // when `canPlayType('application/vnd.apple.mpegurl')` returns
  // anything but `''`". That trapped Chrome on macOS: macOS Chrome
  // returns `"maybe"` (system-level HLS is available via
  // AVFoundation) even though Chrome itself does NOT actually
  // decode HLS through that pathway when MSE is wired up. The video
  // played fine because hls.js was attached as a fallback by the OS,
  // but our handle's `setAudioTrack` was the native-path one which
  // flips `<video>.audioTracks[i].enabled` — a property Chrome
  // never populates for MSE-fed media. Silent no-op, hence "audio
  // doesn't switch on Chrome Tier F".
  //
  // Inverting the check keeps Safari macOS on hls.js (works fine,
  // we lose ~nothing) and gives consistent behaviour across desktop
  // browsers. iOS Safari still falls back to native HLS because
  // `Hls.isSupported()` returns false there.
  const useHlsJs = Hls.isSupported();
  const nativeHls = !useHlsJs &&
    video.canPlayType("application/vnd.apple.mpegurl") !== "";
  console.log(
    `[iris-core] Tier F mount: useHlsJs=${useHlsJs} nativeHls=${nativeHls}`,
  );
  if (nativeHls) {
    video.src = streamUrl;
    return videoBackedHandle(video, {
      nativeTrackMap,
      fallbackDuration: opts.manifest.duration_s ?? null,
      dispose: async () => {
        unbind();
        video.removeEventListener("error", onErr);
        try {
          video.pause();
        } catch {
          /* idempotent */
        }
        video.removeAttribute("src");
        video.load();
      },
      audioTracks: () => collectNativeAudioTracks(video),
      setAudioTrack: (id) => setNativeAudioTrack(video, id),
    });
  }

  if (!useHlsJs) {
    const err = new Error("HLS not supported in this browser (no MSE)");
    opts.onError(err);
    throw err;
  }

  const hls = new Hls({
    xhrSetup: (xhr) => {
      xhr.withCredentials = true;
    },
    // Tight ABR — we serve a single video rendition, so default ABR
    // mostly matters for the audio track switching path.
    debug: false,
    // Capacity tuning: leave defaults; hls.js handles backpressure.
  });
  hls.attachMedia(video);

  hls.on(Hls.Events.MEDIA_ATTACHED, () => {
    hls.loadSource(streamUrl);
  });
  hls.on(Hls.Events.MANIFEST_PARSED, () => {
    opts.onReady?.();
    const tracks = collectHlsAudioTracks(hls);
    console.log(
      `[iris-core] Tier F: HLS manifest parsed. ${tracks.length} audio track(s):`,
      tracks,
      "raw hls.audioTracks =",
      hls.audioTracks,
    );
    // Honor the caller's `audioTrackIndex` on mount. Without this,
    // a demote from Tier B/C/E (where the user had picked, say,
    // audio track 2) lands on Tier F with hls.js's default (0) and
    // the chrome's "active" indicator no longer matches what's
    // actually playing.
    if (
      audioTrackIndex !== undefined &&
      audioTrackIndex >= 0 &&
      audioTrackIndex < hls.audioTracks.length &&
      hls.audioTrack !== audioTrackIndex
    ) {
      console.log(
        `[iris-core] Tier F: applying inherited audio pick ${audioTrackIndex}`,
      );
      hls.audioTrack = audioTrackIndex;
    }
    opts.onAudioTracksChange?.(tracks);
  });
  hls.on(Hls.Events.AUDIO_TRACKS_UPDATED, () => {
    const tracks = collectHlsAudioTracks(hls);
    console.log("[iris-core] Tier F: AUDIO_TRACKS_UPDATED", tracks);
    opts.onAudioTracksChange?.(tracks);
  });
  hls.on(Hls.Events.AUDIO_TRACK_SWITCHED, (_e, data) => {
    console.log("[iris-core] Tier F: AUDIO_TRACK_SWITCHED to id", data.id);
    opts.onAudioTracksChange?.(collectHlsAudioTracks(hls));
  });
  // hls.js media-error recovery. Firefox in particular trips
  // `bufferAppendError` on the first segment append after a seek —
  // the SourceBuffer is sometimes still in `updating` state or has
  // a tiny timing gap that Firefox's MSE rejects but Chrome forgives.
  // The hls.js docs prescribe a 2-step recovery on `mediaError`:
  //   1. `recoverMediaError()` — flushes the SB, re-requests segments.
  //   2. If a second `mediaError` fires within ~3s of step 1,
  //      `swapAudioCodec()` then `recoverMediaError()` again.
  //   3. Only surface the error after step 2 also fails.
  // The recovery state machine is per-mount; it gets reset on
  // success (a brief grace window without a new mediaError).
  let mediaRecoveryAttempt = 0;
  let recoveryTimer: ReturnType<typeof setTimeout> | null = null;
  const armRecoveryReset = () => {
    if (recoveryTimer) clearTimeout(recoveryTimer);
    recoveryTimer = setTimeout(() => {
      mediaRecoveryAttempt = 0;
    }, 5000);
  };
  hls.on(Hls.Events.ERROR, (_event, data) => {
    if (!data.fatal) {
      console.warn("[iris-core] hls.js non-fatal", data.type, data.details);
      return;
    }
    if (data.type === Hls.ErrorTypes.MEDIA_ERROR) {
      if (mediaRecoveryAttempt === 0) {
        console.warn(
          `[iris-core] Tier F: fatal mediaError ${data.details} — recoverMediaError() #1`,
        );
        mediaRecoveryAttempt = 1;
        armRecoveryReset();
        try {
          hls.recoverMediaError();
        } catch (e) {
          opts.onError(e instanceof Error ? e : new Error(String(e)));
        }
        return;
      }
      if (mediaRecoveryAttempt === 1) {
        console.warn(
          `[iris-core] Tier F: fatal mediaError ${data.details} again — swapAudioCodec + recoverMediaError() #2`,
        );
        mediaRecoveryAttempt = 2;
        armRecoveryReset();
        try {
          hls.swapAudioCodec();
          hls.recoverMediaError();
        } catch (e) {
          opts.onError(e instanceof Error ? e : new Error(String(e)));
        }
        return;
      }
      // Recovery exhausted — surface to the demote path.
      const msg = `hls.js fatal ${data.type}: ${data.details} (recovery exhausted)`;
      opts.onError(new Error(msg));
      return;
    }
    if (data.type === Hls.ErrorTypes.NETWORK_ERROR) {
      console.warn(
        `[iris-core] Tier F: fatal networkError ${data.details} — startLoad()`,
      );
      try {
        hls.startLoad();
      } catch (e) {
        opts.onError(e instanceof Error ? e : new Error(String(e)));
      }
      return;
    }
    opts.onError(new Error(`hls.js fatal ${data.type}: ${data.details}`));
  });

  const handle: EngineHandle = videoBackedHandle(video, {
    nativeTrackMap,
    fallbackDuration: opts.manifest.duration_s ?? null,
    dispose: async () => {
      unbind();
      video.removeEventListener("error", onErr);
      if (recoveryTimer) clearTimeout(recoveryTimer);
      try {
        hls.destroy();
      } catch {
        /* idempotent */
      }
      try {
        video.pause();
      } catch {
        /* idempotent */
      }
    },
    audioTracks: () => collectHlsAudioTracks(hls),
    setAudioTrack: (id) => {
      console.log(
        `[iris-core] Tier F: setAudioTrack called id="${id}" hls.audioTracks.length=${hls.audioTracks.length} current=${hls.audioTrack}`,
      );
      const idx = Number(id);
      if (!Number.isFinite(idx)) return;
      const tracks = hls.audioTracks;
      if (idx < 0 || idx >= tracks.length) {
        console.warn(
          `[iris-core] Tier F: setAudioTrack(${idx}) out of range — hls.audioTracks has ${tracks.length} entries`,
        );
        return;
      }
      if (idx === hls.audioTrack) {
        console.log(
          `[iris-core] Tier F: setAudioTrack(${idx}) already active, hls.js no-op`,
        );
        return;
      }
      console.log(
        `[iris-core] Tier F: switching to audio track ${idx} (${tracks[idx]?.name ?? tracks[idx]?.lang ?? "?"})`,
      );
      hls.audioTrack = idx;
    },
  });
  return handle;
};

function collectHlsAudioTracks(hls: Hls): EngineAudioTrack[] {
  return hls.audioTracks.map((t, i) => ({
    id: String(i),
    label: t.name ?? t.lang ?? `Audio ${i + 1}`,
    lang: t.lang ?? undefined,
    active: hls.audioTrack === i,
  }));
}

/** Fallback for Safari native HLS — read the browser's `audioTracks`. */
function collectNativeAudioTracks(video: HTMLVideoElement): EngineAudioTrack[] {
  const nativeTracks = (video as HTMLVideoElement & { audioTracks?: AudioTrackList }).audioTracks;
  if (!nativeTracks) return [];
  const out: EngineAudioTrack[] = [];
  for (let i = 0; i < nativeTracks.length; i += 1) {
    const t = nativeTracks[i];
    if (!t) continue;
    out.push({
      id: t.id || String(i),
      label: t.label || t.language || `Audio ${i + 1}`,
      lang: t.language || undefined,
      active: t.enabled,
    });
  }
  return out;
}

function setNativeAudioTrack(video: HTMLVideoElement, id: string): void {
  const nativeTracks = (video as HTMLVideoElement & { audioTracks?: AudioTrackList }).audioTracks;
  if (!nativeTracks) return;
  for (let i = 0; i < nativeTracks.length; i += 1) {
    const t = nativeTracks[i];
    if (!t) continue;
    t.enabled = t.id === id || String(i) === id;
  }
}

declare global {
  // Some TS lib targets miss AudioTrackList; declare the minimum shape.
  interface AudioTrack {
    id: string;
    label: string;
    language: string;
    kind: string;
    enabled: boolean;
  }
  interface AudioTrackList {
    readonly length: number;
    [index: number]: AudioTrack;
  }
}
