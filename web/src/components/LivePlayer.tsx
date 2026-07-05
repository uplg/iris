import { useEffect, useRef, useState } from "react";
import Hls from "hls.js";

import { livetv } from "@/lib/api";
import { mountLiveAudio, type LiveAudioHandle } from "@/lib/iris-core/live-audio";

/**
 * Live-stream player.
 *
 * **hls.js owns the video** — it is battle-tested against dirty broadcast
 * restreams (variable keyframes, corrupt frames, playlist jitter). It only
 * drops audio codecs MSE can't decode; a TV stream that reaches
 * `BUFFER_CODECS` with no audio track is the E-AC-3/AC-3 signature (browsers
 * have no license for them). We DON'T remux — that proved fragile — instead
 * we start a **WebAudio sidecar** (`live-audio.ts`) that decodes just the
 * audio with libav.js and plays it in sync. Video stays on hls.js.
 *
 * On an unrecoverable hls.js error the served stream is bad: report it to the
 * backend (which cools the source down and elects the next feed) and reload,
 * bounded, then surface the failure banner.
 */
export function LivePlayer({
  src,
  channelName,
  country,
  channelId,
}: {
  src: string;
  channelName: string;
  country: string;
  channelId: string;
}) {
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const [failed, setFailed] = useState(false);
  // Bumping this key re-runs the mount effect — auto source-switch + the
  // "Retry" button both go through it.
  const [attempt, setAttempt] = useState(0);
  const sourceSwitches = useRef(0);
  const countedSrc = useRef(src);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    if (countedSrc.current !== src) {
      // New channel: fresh switch budget.
      countedSrc.current = src;
      sourceSwitches.current = 0;
    }
    setFailed(false);

    let disposed = false;
    let hls: Hls | null = null;
    let audio: LiveAudioHandle | null = null;
    let audioStarted = false;

    /** The served stream is unplayable — tell the backend (demotes the
     *  source, elects the next feed) and remount, bounded. */
    const reportAndSwitch = () => {
      if (disposed) return;
      void livetv.reportPlaybackError(country, channelId).catch(() => {
        /* best-effort */
      });
      if (sourceSwitches.current < 2) {
        sourceSwitches.current += 1;
        console.warn(`[live] switching source (${sourceSwitches.current}/2)`);
        setAttempt((n) => n + 1);
      } else {
        setFailed(true);
      }
    };

    /** hls.js dropped the audio (E-AC-3/AC-3): decode it ourselves via the
     *  WebAudio sidecar and play it in sync with hls.js's video. */
    const startAudioSidecar = (reason: string) => {
      if (disposed || audioStarted || !hls) return;
      audioStarted = true;
      console.info(`[live] starting WebAudio E-AC-3 sidecar: ${reason}`);
      mountLiveAudio(video, hls, src)
        .then((handle) => {
          if (disposed) handle.dispose();
          else audio = handle;
        })
        .catch((e: unknown) => {
          // Audio is best-effort — a failure leaves silent video, not a dead
          // channel.
          console.warn("[live] audio sidecar failed — video stays silent", e);
        });
    };

    if (!Hls.isSupported()) {
      // iOS Safari: native HLS, same-origin so cookies flow by default —
      // and Safari decodes E-AC-3 natively on Apple hardware.
      video.src = src;
      const onErr = () => reportAndSwitch();
      video.addEventListener("error", onErr);
      void video.play().catch(() => {
        /* autoplay may need a tap; controls are visible */
      });
      return () => {
        disposed = true;
        video.removeEventListener("error", onErr);
        video.removeAttribute("src");
        video.load();
      };
    }

    let masterReloads = 0;
    let mediaRecoveries = 0;
    hls = new Hls({
      xhrSetup: (xhr) => {
        xhr.withCredentials = true;
      },
      debug: false,
      renderTextTracksNatively: false,
      // Live: keep both buffers tight — there is no scrubbing, and the
      // stream runs for hours (an unbounded back buffer would OOM the tab,
      // same lesson as Tier F).
      liveDurationInfinity: true,
      backBufferLength: 30,
      maxBufferLength: 30,
      maxMaxBufferLength: 120,
      lowLatencyMode: false,
    });
    hls.on(Hls.Events.MANIFEST_PARSED, () => {
      void video.play().catch(() => {
        /* autoplay may need a tap */
      });
    });
    // The E-AC-3 detector: hls.js only creates buffers for codecs MSE
    // supports. A TV stream that reaches BUFFER_CODECS with no audio track
    // almost certainly carries audio the browser can't decode — start the
    // sidecar so the viewer gets sound.
    hls.on(Hls.Events.BUFFER_CODECS, (_evt, data) => {
      if (!(data as { audio?: unknown }).audio) {
        startAudioSidecar("no MSE-decodable audio track");
      }
    });
    hls.on(Hls.Events.ERROR, (_evt, data) => {
      if (!data.fatal) return;
      if (data.type === Hls.ErrorTypes.NETWORK_ERROR && masterReloads < 3) {
        masterReloads += 1;
        console.warn(`[live] fatal network error — reloading master (${masterReloads}/3)`);
        hls?.loadSource(src);
        return;
      }
      if (data.type === Hls.ErrorTypes.MEDIA_ERROR && mediaRecoveries < 2) {
        mediaRecoveries += 1;
        console.warn(`[live] fatal media error — recoverMediaError (${mediaRecoveries}/2)`);
        hls?.recoverMediaError();
        return;
      }
      // Video itself is unplayable (corrupt stream) — rotate to the next feed.
      console.error("[live] hls.js unrecoverable", data.details);
      reportAndSwitch();
    });
    hls.attachMedia(video);
    hls.loadSource(src);
    return () => {
      disposed = true;
      audio?.dispose();
      hls?.destroy();
    };
  }, [src, attempt, country, channelId]);

  return (
    <div className="relative h-full w-full bg-black">
      <video
        ref={videoRef}
        className="h-full w-full object-contain"
        controls
        playsInline
        aria-label={channelName}
      />
      {failed && (
        <div className="absolute inset-0 grid place-items-center bg-black/80 p-6 text-center">
          <div className="grid justify-items-center gap-3">
            <p className="font-medium text-white">Stream unavailable</p>
            <p className="max-w-sm text-sm text-white/70">
              {channelName} isn't reachable right now — the channel may be geo-blocked, offline,
              or its source is down.
            </p>
            <button
              type="button"
              className="focus-ring rounded-full border border-white/25 px-4 py-1.5 text-sm text-white hover:bg-white/10"
              onClick={() => {
                sourceSwitches.current = 0;
                setAttempt((n) => n + 1);
              }}
            >
              Retry
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
