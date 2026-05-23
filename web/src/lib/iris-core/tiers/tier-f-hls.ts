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

import { isMobileLike } from "../caps";
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
  // Match Vidstack's HLS provider, which leaves `preload` as the
  // empty string default. With `preload="auto"`, Firefox pre-
  // allocates a decoder pipeline before hls.js has even attached
  // its MediaSource — measurably benign on Chrome, mildly
  // problematic on Firefox.
  video.preload = "";
  const nativeTrackMap = new Map<number, HTMLTrackElement>();
  for (const sub of nativeSubs) {
    appendNativeTrack(video, sub, nativeTrackMap);
  }
  container.appendChild(video);

  const initialSeek = { done: false };
  const unbind = bindVideoCallbacks(video, opts, initialSeek);
  // One-shot. Firefox can fire `error` on the `<video>` element
  // tens of times per second when the MSE buffer is full of
  // undecodable data (e.g., HEVC content its VT decoder refuses).
  // Without the guard, every event re-fires `opts.onError` which
  // cascades into demote / banner / analytics POSTs — and once
  // `opts.onError` has surfaced once, repeating doesn't add info.
  let errorFired = false;
  const onErr = () => {
    if (errorFired) return;
    errorFired = true;
    const err = video.error;
    opts.onError(
      new Error(err ? `media error ${err.code}: ${err.message}` : "video element error"),
    );
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
  const nativeHls = !useHlsJs && video.canPlayType("application/vnd.apple.mpegurl") !== "";
  console.log(`[iris-core] Tier F mount: useHlsJs=${useHlsJs} nativeHls=${nativeHls}`);
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

  // Mirror Vidstack's HLS setup (its `HLSController.setup` in
  // `packages/vidstack/src/providers/hls/hls.ts`). The single
  // non-default flag that matters is `renderTextTracksNatively:
  // false`. Default hls.js (`true`) makes hls.js attach itself to
  // any `<track>` element on the video to push HLS-embedded cues
  // into it. We ALREADY append our own `<track>` elements for the
  // server-side .vtt subtitle URLs (see `appendNativeTrack` in
  // engine.ts) — when hls.js then competes for those tracks, the
  // resulting TextTrack state churn upsets Firefox's MSE pipeline
  // enough that AppleVTDecoder errors out on the next segment
  // append. Chrome tolerates it; Firefox doesn't.
  //
  // Switching to `renderTextTracksNatively: false` tells hls.js to
  // mind its own business: any HLS-internal text tracks would be
  // exposed via the `NON_NATIVE_TEXT_TRACKS_FOUND` event (we don't
  // listen for it because our HLS pipeline never embeds subs —
  // shaka-packager serves them as standalone files referenced from
  // the manifest), and our `<track>` elements remain untouched.
  // Memory ceiling. hls.js defaults `backBufferLength` to Infinity —
  // it NEVER evicts segments behind the playhead, so a 2-hour film
  // grows the SourceBuffer monotonically until the tab is OOM-killed
  // (mobile Chrome's "Aw, Snap!"). Since Tier F is now the universal
  // mobile fallback (see `pickTier`), an unbounded back buffer would
  // just relocate the crash here. We cap the back buffer hard, and on
  // phones/tablets also tighten the forward buffer so the live
  // footprint stays well under the per-tab budget. Desktop keeps a
  // roomier forward buffer for scrub resilience.
  const mobile = isMobileLike();
  const hls = new Hls({
    xhrSetup: (xhr) => {
      xhr.withCredentials = true;
    },
    debug: false,
    renderTextTracksNatively: false,
    // Evict played-out media; 30 s of scrub-back is plenty.
    backBufferLength: 30,
    // Forward buffer caps. Mobile gets a tighter ceiling (both the
    // duration and the absolute byte size) to keep the renderer alive
    // across a full feature-length playback.
    maxBufferLength: mobile ? 20 : 30,
    maxMaxBufferLength: mobile ? 60 : 600,
    maxBufferSize: mobile ? 20 * 1000 * 1000 : 60 * 1000 * 1000,
  });

  // Match Vidstack's HLSController.setup ordering exactly:
  //   1. Register every event listener BEFORE attachMedia.
  //   2. Call attachMedia.
  //   3. Set up DOM (preload + a `<source>` element with the
  //      manifest URL — hls.js removes all `<source>` children on
  //      attach and re-adds its own blob source, so this only
  //      matters as an AirPlay hint; we mirror it for safety).
  //   4. Call loadSource directly (no waiting on MEDIA_ATTACHED —
  //      hls.js queues it internally until attach completes).
  //
  // Empirical: Firefox's media pipeline is sensitive to subtle
  // ordering here. Registering listeners AFTER attachMedia plus
  // gating loadSource on MEDIA_ATTACHED is what we had before;
  // Vidstack didn't do either and worked. Aligning gives us the
  // strongest chance the next test crosses the threshold.
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
      console.log(`[iris-core] Tier F: applying inherited audio pick ${audioTrackIndex}`);
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
  // Error handling, mirrored from Vidstack's `HLSController.#onError`
  // but bounded. `hls.recoverMediaError()` detaches the media
  // element and re-loads the manifest from scratch; when the
  // underlying issue is content-shaped (Firefox + 4K HEVC HDR, a
  // bsf strip ate something VT needed, …) the recovery itself
  // trips the same fatal `bufferAppendError` within seconds and we
  // loop forever — the user sees "Tier F: HLS manifest parsed"
  // every couple seconds. Cap at `MAX_RECOVER` attempts inside a
  // `RECOVER_WINDOW_MS` sliding window; past that, surface the
  // error AND stop the loader so subsequent error events don't
  // re-fire `opts.onError` (which would re-set the banner on every
  // tick) or keep streaming bytes from the server.
  const MAX_RECOVER = 2;
  const RECOVER_WINDOW_MS = 8_000;
  const recentRecoveries: number[] = [];
  let surfaced = false;
  hls.on(Hls.Events.ERROR, (_event, data) => {
    if (surfaced) {
      // Already gave up. Swallow further fatal errors so we don't
      // re-fire `opts.onError` and re-trigger the banner / demote
      // path on every subsequent tick.
      return;
    }
    if (!data.fatal) {
      console.warn("[iris-core] hls.js non-fatal", data.type, data.details);
      return;
    }
    if (data.type === Hls.ErrorTypes.MEDIA_ERROR) {
      const now = performance.now();
      while (recentRecoveries.length > 0 && now - recentRecoveries[0]! > RECOVER_WINDOW_MS) {
        recentRecoveries.shift();
      }
      if (recentRecoveries.length >= MAX_RECOVER) {
        surfaced = true;
        console.error(
          `[iris-core] Tier F: ${recentRecoveries.length} fatal recoveries in ${RECOVER_WINDOW_MS}ms — surfacing ${data.details}`,
        );
        // Stop the loader; `hls.destroy()` would be ideal but it
        // can re-enter this handler with its own teardown errors,
        // so we go for the lighter `stopLoad()` + `detachMedia()`.
        // Full `destroy()` runs in the engine's `dispose()` path
        // once IrisPlayer / WatchPage react to the surfaced error.
        try {
          hls.stopLoad();
        } catch {
          /* idempotent */
        }
        try {
          hls.detachMedia();
        } catch {
          /* idempotent */
        }
        opts.onError(new Error(`hls.js fatal ${data.type}: ${data.details}`));
        return;
      }
      recentRecoveries.push(now);
      console.warn(
        `[iris-core] Tier F: fatal mediaError ${data.details} — recoverMediaError() #${recentRecoveries.length}`,
      );
      try {
        hls.recoverMediaError();
      } catch (e) {
        surfaced = true;
        try {
          hls.stopLoad();
        } catch {
          /* idempotent */
        }
        opts.onError(e instanceof Error ? e : new Error(String(e)));
      }
      return;
    }
    surfaced = true;
    try {
      hls.stopLoad();
    } catch {
      /* idempotent */
    }
    opts.onError(new Error(`hls.js fatal ${data.type}: ${data.details}`));
  });

  // Now attach + load. Order taken from Vidstack: listeners are
  // registered above, then `attachMedia`, then `loadSource` directly.
  hls.attachMedia(video);
  // Mirror Vidstack's `appendSource` for AirPlay hint compatibility.
  // hls.js's `BufferController.onMediaAttaching` strips all
  // `<source>` children and re-adds its own `blob:` source pointing
  // at the MediaSource, so this is purely informational once
  // attached — but adding it makes the pre-attach video element
  // state match what Vidstack's player passed to hls.js.
  const hlsSource = document.createElement("source");
  hlsSource.src = streamUrl;
  hlsSource.type = "application/x-mpegurl";
  hlsSource.setAttribute("data-iris", "");
  video.appendChild(hlsSource);
  hls.loadSource(streamUrl);

  const handle: EngineHandle = videoBackedHandle(video, {
    nativeTrackMap,
    fallbackDuration: opts.manifest.duration_s ?? null,
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
        console.log(`[iris-core] Tier F: setAudioTrack(${idx}) already active, hls.js no-op`);
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
