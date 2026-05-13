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
  bindVideoCallbacks,
  videoBackedHandle,
  type EngineAudioTrack,
  type EngineHandle,
  type EngineMount,
} from "../engine";

export const mountTierF: EngineMount = async (opts) => {
  const { container, streamUrl, nativeSubs } = opts;
  container.innerHTML = "";
  const video = document.createElement("video");
  video.className = "h-full w-full object-contain";
  video.playsInline = true;
  video.preload = "auto";
  for (const sub of nativeSubs) {
    const track = document.createElement("track");
    track.src = sub.vttUrl;
    track.kind = "subtitles";
    track.label = sub.title ?? sub.lang?.toUpperCase() ?? `Sub ${sub.stream_idx}`;
    track.srclang = sub.lang ?? "und";
    if (sub.default) track.default = true;
    video.appendChild(track);
  }
  container.appendChild(video);

  const initialSeek = { done: false };
  const unbind = bindVideoCallbacks(video, opts, initialSeek);
  const onErr = () => {
    const err = video.error;
    opts.onError(new Error(err ? `media error ${err.code}: ${err.message}` : "video element error"));
  };
  video.addEventListener("error", onErr);

  // Safari (macOS / iOS) plays HLS natively without `hls.js`. Detect
  // and short-circuit — Safari's native pipeline handles fMP4 HLS
  // with multi-audio + WebVTT subs out of the box.
  const nativeHls = video.canPlayType("application/vnd.apple.mpegurl") !== "";
  if (nativeHls) {
    video.src = streamUrl;
    return videoBackedHandle(video, {
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

  if (!Hls.isSupported()) {
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
    opts.onAudioTracksChange?.(collectHlsAudioTracks(hls));
  });
  hls.on(Hls.Events.AUDIO_TRACKS_UPDATED, () => {
    opts.onAudioTracksChange?.(collectHlsAudioTracks(hls));
  });
  hls.on(Hls.Events.AUDIO_TRACK_SWITCHED, () => {
    opts.onAudioTracksChange?.(collectHlsAudioTracks(hls));
  });
  hls.on(Hls.Events.ERROR, (_event, data) => {
    if (data.fatal) {
      const msg = `hls.js fatal ${data.type}: ${data.details}`;
      opts.onError(new Error(msg));
    } else {
      console.warn("[iris-core] hls.js non-fatal", data.type, data.details);
    }
  });

  const handle: EngineHandle = videoBackedHandle(video, {
    dispose: async () => {
      unbind();
      video.removeEventListener("error", onErr);
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
      const idx = Number(id);
      if (Number.isFinite(idx)) hls.audioTrack = idx;
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
