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
import { mountLiveAudio, type LiveAudioHandle } from "../live-audio";
import type { Manifest } from "../manifest-client";

export const mountTierF: EngineMount = async (opts) => {
  const { container, streamUrl, nativeSubs, audioTrackIndex } = opts;
  const live = opts.live === true;
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
    // Parity with the hls.js branch's MANIFEST_PARSED hook: apply the
    // inherited audio pick once Safari has populated `audioTracks`
    // (empty before metadata), so a remount / tier demote keeps the
    // user's language instead of snapping back to the default.
    if (audioTrackIndex !== undefined && audioTrackIndex >= 0) {
      video.addEventListener(
        "loadedmetadata",
        () => setNativeAudioTrack(video, String(audioTrackIndex), opts.manifest.audio),
        { once: true },
      );
    }
    if (live) {
      // Live channels autoplay (the channel click IS the intent). Safari
      // decodes E-AC-3 natively on Apple hardware, so no sidecar here.
      void video.play().catch(() => {
        /* autoplay may need a tap; the chrome's play button is visible */
      });
    }
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
      setAudioTrack: (id) => setNativeAudioTrack(video, id, opts.manifest.audio),
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
    // Forward buffer caps. Live keeps both buffers tight — there is no
    // scrubbing and the stream runs for hours (an unbounded buffer would
    // OOM the tab). VOD: mobile gets a tighter ceiling (both the duration
    // and the absolute byte size) to keep the renderer alive across a
    // full feature-length playback.
    ...(live
      ? {
          liveDurationInfinity: true,
          maxBufferLength: 30,
          maxMaxBufferLength: 120,
          lowLatencyMode: false,
        }
      : {
          maxBufferLength: mobile ? 20 : 30,
          maxMaxBufferLength: mobile ? 60 : 600,
          maxBufferSize: mobile ? 20 * 1000 * 1000 : 60 * 1000 * 1000,
        }),
  });

  // Live-only state: the E-AC-3 WebAudio sidecar and the bounded
  // master-reload budget for fatal network errors (a dying upstream 502s
  // for a beat while the backend cools it down and elects the next feed).
  let disposed = false;
  let liveAudio: LiveAudioHandle | null = null;
  let liveAudioStarted = false;
  let liveMasterReloads = 0;
  if (live) {
    // The E-AC-3 detector. hls.js only creates SourceBuffers for codecs MSE
    // can decode, so "the stream is playing and there is still no audio
    // buffer" is the signal that the feed carries audio the browser refuses
    // (E-AC-3/AC-3 in a TS feed) and we have to decode it ourselves through
    // the WebAudio sidecar.
    //
    // The subtlety that bit us: BUFFER_CODECS fires ONCE PER STREAM
    // CONTROLLER. With an alternate audio rendition the main controller
    // announces `video` first and the audio controller announces `audio` a
    // beat later. Deciding on that first video-only event put the sidecar
    // and the element's own audio on the speakers simultaneously — the
    // double-audio bug.
    //
    // So the decision is made on accumulated state, deferred until fragments
    // are actually landing, and REVERSIBLE: an audio buffer showing up late
    // retracts it and disposes the sidecar instead of doubling.
    const buffers = new Set<string>();
    const hasMseAudio = () => buffers.has("audio") || buffers.has("audiovideo");
    /** Fragments buffered before we conclude no audio buffer is coming. Two
     *  is past the point where an alt-audio controller would have announced
     *  its own codecs. */
    const DECIDE_AFTER_FRAGS = 2;
    let fragsBuffered = 0;

    const startSidecar = () => {
      liveAudioStarted = true;
      console.info("[iris-core] Tier F live: no MSE-decodable audio — starting WebAudio sidecar");
      mountLiveAudio(video, hls, streamUrl)
        .then((h) => {
          // `disposed` covers engine teardown; `hasMseAudio()` covers hls.js
          // announcing an audio buffer while we were mounting.
          if (disposed || hasMseAudio()) h.dispose();
          else liveAudio = h;
        })
        .catch((e: unknown) => {
          // Audio is best-effort — a failure leaves silent video, not a
          // dead channel.
          console.warn("[iris-core] Tier F live: audio sidecar failed — video stays silent", e);
        });
    };

    hls.on(Hls.Events.BUFFER_CODECS, (_evt, data) => {
      for (const name of Object.keys(data)) buffers.add(name);
      if (!hasMseAudio() || disposed) return;
      // The element plays its own audio after all — retract.
      if (liveAudio || liveAudioStarted) {
        console.info(
          `[iris-core] Tier F live: MSE audio buffer appeared (${[...buffers].join("+")}) — dropping the sidecar`,
        );
        liveAudio?.dispose();
        liveAudio = null;
      }
    });

    hls.on(Hls.Events.FRAG_BUFFERED, () => {
      if (disposed || liveAudioStarted || hasMseAudio()) return;
      fragsBuffered += 1;
      if (fragsBuffered >= DECIDE_AFTER_FRAGS) startSidecar();
    });
  }

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
    if (live) {
      void video.play().catch(() => {
        /* autoplay may need a tap; the chrome's play button is visible */
      });
    }
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
  // Error handling. hls.js 1.7 ships its OWN recovery machinery: the
  // `ErrorController` listens for `ERROR` from the Hls constructor — so
  // before this handler — attaches a plan to `data.errorAction`, and applies
  // it: switching level, penalty-boxing an alternate, and, when the plan
  // carries the `ResetMediaSource` flag (`mediaSourceRequiresReset`), calling
  // `hls.recoverMediaError()` itself. It marks the plan `resolved` when it
  // worked, and only leaves `data.fatal` set when it ran out of options —
  // in which case it has already stopped the loader.
  //
  // Our 1.6-era handler called `recoverMediaError()` unconditionally on every
  // fatal media error, which under 1.7 means a SECOND detach/re-attach cycle
  // racing the one hls.js just started — manufacturing exactly the
  // `bufferAppendError` / `mediaSourceRequiresReset` storm it was supposed to
  // clear, and then reporting a perfectly healthy feed as broken (the backend
  // cools a reported source down household-wide). So: let hls.js drive, and
  // only act on what it declares unrecoverable.
  //
  // `ErrorActionFlags` is a `const enum` in hls.js's .d.ts, which
  // `isolatedModules` forbids importing — mirror the one value we read.
  const FLAG_RESET_MEDIA_SOURCE = 16; // ErrorActionFlags.ResetMediaSource
  // Self-recovery attempts for a fatal media error hls.js did NOT already
  // reset the MediaSource for. One is enough: if the first re-attach doesn't
  // take, the content is the problem and rotating beats looping.
  const MAX_RECOVER = 1;
  let recoveries = 0;
  let surfaced = false;

  // `recoverMediaError()` — ours or hls.js's — detaches and re-attaches the
  // media element, which RESETS `currentTime` to 0. Mid-film that silently
  // restarted the user from the beginning. Restore the position on the first
  // `canplay` after the reset: one-shot, event-driven, and a no-op both when
  // hls.js kept the position and on live (where re-joining at the live edge
  // is the desired outcome, not a regression).
  const armPlayheadRestore = () => {
    if (live) return;
    const resumeAt = video.currentTime;
    if (resumeAt <= 1) return;
    const restore = () => {
      video.removeEventListener("canplay", restore);
      if (video.currentTime < resumeAt - 1) {
        console.warn(
          `[iris-core] Tier F: post-recovery playhead at ${video.currentTime.toFixed(1)}s — restoring ${resumeAt.toFixed(1)}s`,
        );
        try {
          video.currentTime = resumeAt;
        } catch {
          /* element torn down — dispose path owns it */
        }
      }
    };
    video.addEventListener("canplay", restore);
  };

  const giveUp = (message: string) => {
    surfaced = true;
    // Stop the loader so subsequent error events don't re-fire
    // `opts.onError` (which would re-set the banner / re-demote the source on
    // every tick) or keep streaming bytes from the server. `hls.destroy()`
    // would be ideal but it can re-enter this handler with its own teardown
    // errors; full `destroy()` runs in the engine's `dispose()` once
    // IrisPlayer / the page react to the surfaced error.
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
    opts.onError(new Error(message));
  };

  hls.on(Hls.Events.ERROR, (_event, data) => {
    if (surfaced) {
      // Already gave up. Swallow the rest.
      return;
    }
    const action = data.errorAction;
    const resetsMediaSource = ((action?.flags ?? 0) & FLAG_RESET_MEDIA_SOURCE) !== 0;
    if (!data.fatal) {
      // hls.js is handling it — including, when the flag is set, a media
      // source reset it performs itself.
      if (resetsMediaSource) armPlayheadRestore();
      console.warn("[iris-core] hls.js non-fatal", data.type, data.details, action?.resolved);
      return;
    }

    // Fatal: hls.js could not resolve it and has stopped the loader.
    if (live && data.type === Hls.ErrorTypes.NETWORK_ERROR && liveMasterReloads < 3) {
      // A dying upstream 502s for a beat while the backend cools it down and
      // elects the next feed — reloading the master picks up that election.
      liveMasterReloads += 1;
      console.warn(
        `[iris-core] Tier F live: fatal network error — reloading master (${liveMasterReloads}/3)`,
      );
      hls.loadSource(streamUrl);
      return;
    }
    if (
      data.type === Hls.ErrorTypes.MEDIA_ERROR &&
      !resetsMediaSource &&
      recoveries < MAX_RECOVER
    ) {
      recoveries += 1;
      console.warn(
        `[iris-core] Tier F: fatal mediaError ${data.details} — recoverMediaError() #${recoveries}`,
      );
      armPlayheadRestore();
      try {
        hls.recoverMediaError();
        return;
      } catch (e) {
        giveUp(e instanceof Error ? e.message : String(e));
        return;
      }
    }
    console.error(`[iris-core] Tier F: unrecoverable ${data.type} / ${data.details}`);
    giveUp(`hls.js fatal ${data.type}: ${data.details}`);
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
      disposed = true;
      liveAudio?.dispose();
      liveAudio = null;
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

/** Fallback for Safari native HLS — read the browser's `audioTracks`.
 *  Ids are POSITIONAL (`"0"`, `"1"`, …) to live in the same namespace as
 *  the chrome's manifest-derived menu ids. Never expose Safari's own
 *  `AudioTrack.id`: it's 1-based ("1", "2", …), so menu id "1" used to
 *  match BOTH Safari's first track (by id) and the second (by position),
 *  enabling two tracks at once — the "picked Korean, got French" bug. */
function collectNativeAudioTracks(video: HTMLVideoElement): EngineAudioTrack[] {
  const nativeTracks = (video as HTMLVideoElement & { audioTracks?: AudioTrackList }).audioTracks;
  if (!nativeTracks) return [];
  const out: EngineAudioTrack[] = [];
  for (let i = 0; i < nativeTracks.length; i += 1) {
    const t = nativeTracks[i];
    if (!t) continue;
    out.push({
      id: String(i),
      label: t.label || t.language || `Audio ${i + 1}`,
      lang: t.language || undefined,
      active: t.enabled,
    });
  }
  return out;
}

/** ISO 639-2 (ffprobe / `manifest.audio[].lang`) → 639-1 (what Safari
 *  reports from the playlist's `LANGUAGE` attribute, normalised by
 *  shaka-packager). Mirrors the server's `iso639_2to1` in remuxer.rs. */
const ISO639_2TO1: Record<string, string> = {
  fre: "fr",
  fra: "fr",
  eng: "en",
  spa: "es",
  ger: "de",
  deu: "de",
  ita: "it",
  por: "pt",
  rus: "ru",
  jpn: "ja",
  kor: "ko",
  chi: "zh",
  zho: "zh",
  ara: "ar",
  dut: "nl",
  nld: "nl",
  pol: "pl",
  swe: "sv",
  tur: "tr",
  ukr: "uk",
  heb: "he",
  hin: "hi",
  vie: "vi",
  ces: "cs",
  cze: "cs",
  dan: "da",
  fin: "fi",
  nor: "no",
  ron: "ro",
  rum: "ro",
  gre: "el",
  ell: "el",
};

function normalizeLang(lang: string | null | undefined): string | null {
  if (!lang) return null;
  const primary = lang.toLowerCase().split("-")[0] ?? "";
  if (primary === "" || primary === "und") return null;
  return ISO639_2TO1[primary] ?? primary;
}

/** `id` is an index into `manifest.audio` (the chrome menu's namespace).
 *  Safari's `AudioTrackList` order for native HLS is not guaranteed to
 *  match the playlist's `EXT-X-MEDIA` order, so resolve by language
 *  first (unambiguous when each track has a distinct language) and fall
 *  back to position. Exactly one track ends up enabled. */
function setNativeAudioTrack(
  video: HTMLVideoElement,
  id: string,
  manifestAudio: Manifest["audio"],
): void {
  const nativeTracks = (video as HTMLVideoElement & { audioTracks?: AudioTrackList }).audioTracks;
  if (!nativeTracks) return;
  const idx = Number(id);
  if (!Number.isFinite(idx) || idx < 0) return;
  let target = idx;
  const wantedLang = normalizeLang(manifestAudio[idx]?.lang);
  if (wantedLang) {
    const matches: number[] = [];
    for (let i = 0; i < nativeTracks.length; i += 1) {
      if (normalizeLang(nativeTracks[i]?.language) === wantedLang) matches.push(i);
    }
    if (matches.length === 1) target = matches[0] ?? idx;
  }
  console.log(
    `[iris-core] Tier F native: setAudioTrack id=${id} lang=${wantedLang ?? "?"} → native index ${target}`,
  );
  for (let i = 0; i < nativeTracks.length; i += 1) {
    const t = nativeTracks[i];
    if (!t) continue;
    t.enabled = i === target;
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
