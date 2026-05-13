/**
 * Tier A — native `<video src=/stream type=video/mp4>` with HTTP Range.
 *
 * The simplest engine: vanilla `<video>` pointed at the server's raw
 * stream endpoint. The browser handles demux + decode entirely.
 * Used when the source is fMP4 with HW-decodable codecs.
 */

import {
  bindVideoCallbacks,
  videoBackedHandle,
  type EngineHandle,
  type EngineMount,
} from "../engine";

export const mountTierA: EngineMount = async (opts) => {
  const { container, streamUrl, nativeSubs } = opts;
  container.innerHTML = "";
  const video = document.createElement("video");
  video.className = "h-full w-full object-contain";
  video.playsInline = true;
  // No autoplay — Firefox/Safari block with-sound autoplay.
  video.preload = "auto";
  // `crossOrigin` defaults to anonymous on same-origin requests; we
  // serve /stream same-origin, so no extra config needed.
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
  video.src = streamUrl;

  const initialSeek = { done: false };
  const unbind = bindVideoCallbacks(video, opts, initialSeek);

  let firstPlayable = false;
  const onLoadedData = () => {
    if (firstPlayable) return;
    firstPlayable = true;
    opts.onReady?.();
  };
  video.addEventListener("loadeddata", onLoadedData);

  const onErr = () => {
    const err = video.error;
    const msg = err ? `media error ${err.code}: ${err.message}` : "video element error";
    opts.onError(new Error(msg));
  };
  video.addEventListener("error", onErr);

  const handle: EngineHandle = videoBackedHandle(video, {
    dispose: async () => {
      unbind();
      video.removeEventListener("loadeddata", onLoadedData);
      video.removeEventListener("error", onErr);
      try {
        video.pause();
      } catch {
        /* idempotent */
      }
      video.removeAttribute("src");
      video.load();
    },
  });
  return handle;
};
