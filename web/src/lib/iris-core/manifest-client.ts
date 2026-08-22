/**
 * `Iris-Caps`-aware manifest client.
 *
 * Wraps the server's `/api/torrents/{infohash}/files/{idx}/manifest.json`
 * endpoint and the matching capability declaration. See
 * docs/SOTA_ARCHITECTURE.md §2.1 for the wire format. The TS types here are
 * the dual of `iris-media::manifest::Manifest` in Rust.
 */

import type { components } from "../api-types";
import { capsHeader, hevcMseNeedsIdrStart, isMobileLike, probeCapabilities } from "./caps";
import { libavCanDecode } from "./decode/libav-audio-decoder";
import { cheapProbeVideoCodec } from "./decode/webcodecs-probe";

// The manifest wire format is owned by the Rust `iris-media::manifest`
// module and emitted into the OpenAPI contract; these are thin aliases over
// the generated schema so a backend field change shows up here at `tsc`.
export type HdrKind = components["schemas"]["HdrKind"];
export type ByteRange = components["schemas"]["ByteRange"];
export type DownloadStatus = components["schemas"]["DownloadStatus"];
export type VideoTrack = components["schemas"]["VideoTrack"];
export type AudioTrack = components["schemas"]["AudioTrack"];
export type SubtitleTrack = components["schemas"]["SubtitleTrack"];
export type Chapter = components["schemas"]["Chapter"];
export type Manifest = components["schemas"]["Manifest"];

/**
 * The decode-tier label returned by `pickTier`.
 *
 * - **A**: direct `<video src>` over /stream — fMP4 + all codecs HW-native.
 * - **B**: Mediabunny demux + remux to fMP4 → MSE — non-MP4 container
 *   (MKV / AVI / WebM / TS) with browser-native codecs.
 * - **C**: WebCodecs hardware decode → Canvas2D/WebGPU. The route for
 *   codecs MSE refuses but the GPU still decodes (HEVC on Chrome
 *   Linux/macOS, MKV+HEVC anywhere).
 * - **D**: WebCodecs software decode (Chrome's bundled dav1d / libvpx /
 *   openh264) → Canvas2D. Same code path as C, different probe outcome.
 * - **E**: hevc.js MSE intercept — HEVC → H.264 transcode in a WASM
 *   worker. Picked when WebCodecs HEVC isn't available (Chrome Linux
 *   without HW HEVC, Firefox). Gated to 1080p.
 * - **F**: legacy server-side HLS remux. Final fallback.
 */
export type DecodeTier = "A" | "B" | "C" | "D" | "E" | "F";

/**
 * Result thrown via `Promise.reject` when the server reports the file is
 * still downloading. The caller is expected to retry after a short delay.
 */
export class ManifestNotReadyError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ManifestNotReadyError";
  }
}

export async function fetchManifest(infohash: string, fileIdx: number): Promise<Manifest> {
  const caps = await probeCapabilities();
  const res = await fetch(`/api/torrents/${infohash}/files/${fileIdx}/manifest.json`, {
    credentials: "include",
    headers: { "Iris-Caps": capsHeader(caps) },
  });
  if (res.status === 400) {
    // Phase 0: the server returns 400 + "file not yet on disk" / "download
    // in progress" until the torrent finishes. Phase 1 will switch this to
    // a streaming manifest, but for now we surface a distinct error so the
    // caller can keep polling without bouncing the user to an error page.
    const body = (await res.json().catch(() => null)) as { message?: string } | null;
    const message = body?.message ?? "manifest not ready";
    if (/download in progress|file not yet on disk|not yet probable/i.test(message)) {
      throw new ManifestNotReadyError(message);
    }
  }
  if (!res.ok) {
    const body = (await res.json().catch(() => null)) as { message?: string } | null;
    throw new Error(body?.message ?? `manifest fetch failed (${res.status})`);
  }
  return (await res.json()) as Manifest;
}

/**
 * Pick a decode tier from a manifest.
 *
 * Phase 2a wires A and B; C–E land Phase 2b+. F is the last-resort
 * fallback that always works because the server runs the legacy HLS
 * remux pipeline.
 */
export async function pickTier(manifest: Manifest): Promise<DecodeTier> {
  if (typeof globalThis.MediaSource === "undefined") return "F";
  if (manifest.video.length === 0) return "F";

  const codecsMse = manifest.video.every((v) => {
    const mime = v.codec_string ? `video/mp4; codecs="${v.codec_string}"` : `video/mp4`;
    return MediaSource.isTypeSupported(mime);
  });
  const audioNative = manifest.audio.every((a) => a.browser_native);
  // libav.js + WebCodecs.AudioEncoder lets us transcode AC-3, E-AC-3
  // and FLAC to AAC client-side at mount time (see Tier B's audio
  // filter). Tracks outside that set fall to Tier F.
  const audioTranscodable = manifest.audio.every(
    (a) => a.browser_native || libavCanDecode(a.codec),
  );
  if (!audioNative && !audioTranscodable) return "F";

  // Mobile gate. On phones/tablets we keep the engines whose memory
  // footprint the *browser* bounds for us — Tier A (native `<video
  // src>`), Tier B (Mediabunny demux → MSE; its SourceBuffer is hard-
  // capped at ~30 s ahead / 15 s behind on mobile, see tier-b-mse) and
  // Tier F (server-side HLS, back-buffer bounded). We refuse the heap-
  // heavy ones: C/D decode into WebCodecs `VideoFrame`s and render to a
  // canvas (several MB per frame, in JS-managed queues), and E spins a
  // second HEVC→H.264 WASM transcoder. Those accumulate enough to trip
  // mobile Chrome's OOM killer mid-film, which kills the renderer with
  // NO JS error — so the demote cascade can't recover and the only
  // defence is to never pick them. Crucially, C/D/E only ever apply
  // when MSE has already *refused* the codec, so there's no remux to
  // "dodge" on this path anyway: the safe alternative is the server
  // HLS remux (F). Tier B, by contrast, IS the way we dodge remux for
  // native-codec MKV — keep it. (Desktop keeps the full cascade below.)
  const isMp4Family = /mp4|mov|m4v|isobmff/i.test(manifest.container);
  if (isMobileLike()) {
    if (codecsMse && audioNative && isMp4Family) return "A";
    // `audioTranscodable` is guaranteed true here (the `!audioNative &&
    // !audioTranscodable` early-return above, plus `audioNative ⟹
    // audioTranscodable`), so any MSE-decodable video lands on B.
    if (codecsMse && audioTranscodable) return "B";
    return "F";
  }

  // HEVC where MSE will only ever start on an IDR (Firefox-family on macOS,
  // Gecko 154+ — see `hevcMseNeedsIdrStart`). Tier B would play from t=0 and
  // then die on the first seek or resume, because an open-GOP rip carries a
  // single IDR at the head and every later keyframe is a CRA that this engine
  // refuses to open a coded frame group on. Route to hevc.js instead: it
  // transcodes to H.264 in a WASM worker, so what reaches MSE is a codec with
  // no such restriction. Above 1080p hevc.js runs at ~21 fps, so the server
  // remux (F) is the honest answer there.
  //
  // This sits BEFORE the Tier A/B branches on purpose: `codecsMse` is true for
  // `hev1.*` on these builds — `isTypeSupported` says yes and the demuxer then
  // drops the frames — so B would otherwise win and fail later.
  const hevcPrimary = manifest.video[0];
  if (
    hevcPrimary &&
    /hevc|hev1|hvc1|h265|x265/i.test(hevcPrimary.codec) &&
    hevcMseNeedsIdrStart()
  ) {
    return (hevcPrimary.height ?? 0) <= 1080 ? "E" : "F";
  }

  // Tier A: must be MSE-friendly AND native audio (we can't inject
  // libav.js into a vanilla `<video src>` — the engine is the
  // browser, no hooks). Tier B picks up the libav-transcoded case.
  if (codecsMse && audioNative) {
    if (isMp4Family) return "A";
    const tierBContainers = /matroska|webm|avi|mpegts|quicktime|mov/i;
    if (tierBContainers.test(manifest.container)) return "B";
  }

  // Tier B with audio transcode: MSE accepts the video, libav.js
  // transcodes the audio to AAC. We always end up in Tier B here
  // (MP4-family containers with non-native audio also work — the
  // remux just rebuilds the audio rendition).
  if (codecsMse && audioTranscodable) {
    return "B";
  }

  // Tier C/D: codec MSE refuses but WebCodecs might decode it. Walk
  // the primary video track; if its codec_string passes the cheap
  // isConfigSupported check, we're a candidate. The real 1-frame
  // test runs at mount time so we don't pay it on every manifest
  // fetch.
  const primary = manifest.video[0];
  if (primary?.codec_string) {
    const probe = await cheapProbeVideoCodec(primary.codec_string);
    if (probe.supportedHardware) return "C";
    if (probe.supportedAny) return "D";
  }

  // Tier E: HEVC at ≤ 1080p where neither MSE nor WebCodecs accept the codec.
  // hevc.js transcodes to H.264 in a WASM worker. 4K is excluded because
  // hevc.js hits ~21 fps there.
  //
  // No longer Chromium-only: that gate was hedging against Firefox lacking
  // WebCodecs H.264 encode. Measured on Gecko 154 — `VideoEncoder
  // .isConfigSupported` returns true for avc1 baseline, main AND high at
  // 1920x960, `VideoDecoder` likewise, and MSE accepts `avc1.640028` with both
  // `opus` and `mp4a.40.2`. Mobile stays excluded by the gate far above (the
  // WASM transcoder is the heap-heavy engine that trips mobile OOM).
  if (primary && /hevc|hev1|hvc1|h265|x265/i.test(primary.codec) && (primary.height ?? 0) <= 1080) {
    if (typeof VideoEncoder !== "undefined") return "E";
  }

  return "F";
}

/** Build the direct-stream URL (used by Tier A and Tier B). */
export function rawStreamUrl(infohash: string, fileIdx: number): string {
  return `/api/torrents/${infohash}/files/${fileIdx}/stream`;
}

/** Legacy HLS master URL (Tier F). */
export function hlsUrl(infohash: string, fileIdx: number): string {
  return `/api/torrents/${infohash}/files/${fileIdx}/play/master.m3u8`;
}

/**
 * URL for the `.vtt` extraction of a subtitle track, regardless of the
 * source codec. The server transcodes text-based codecs (`subrip`,
 * `ass`, `mov_text`…) to WebVTT for the native `<track>` path.
 *
 * Use this URL — not `track.url` — when injecting `<track>` elements.
 * `track.url` follows the source codec extension (`.ass`, `.sup`) and
 * is meant for the libass / libpgs overlay paths.
 */
export function nativeSubtitleUrl(manifest: Manifest, streamIdx: number): string {
  return `/api/torrents/${manifest.infohash}/files/${manifest.file_idx}/sub/${streamIdx}/track.vtt`;
}

/** Build the playback URL for the chosen tier (compat shim, prefer the
 *  tier-specific helpers above). */
export function streamUrl(infohash: string, fileIdx: number, tier: DecodeTier): string {
  if (tier === "F") return hlsUrl(infohash, fileIdx);
  return rawStreamUrl(infohash, fileIdx);
}

/**
 * Tell the server which byte the playhead just moved to, so its piece
 * picker can prioritise the next ~30 seconds of media. Fire-and-forget;
 * we never await the response. Translates the seek from seconds to
 * bytes using the manifest's duration + size (assumes near-constant
 * bitrate — good enough for piece priority hinting).
 */
export function postSeekHint(manifest: Manifest, playheadSeconds: number): void {
  if (manifest.duration_s == null || manifest.duration_s <= 0) return;
  const ratio = Math.max(0, Math.min(1, playheadSeconds / manifest.duration_s));
  const byteOffset = Math.floor(ratio * manifest.size_bytes);
  const url = `/api/torrents/${manifest.infohash}/files/${manifest.file_idx}/seek`;
  const body = JSON.stringify({ byte_offset: byteOffset, playhead_s: playheadSeconds });
  // Use keepalive so a fast subsequent navigation doesn't cancel the hint.
  void fetch(url, {
    method: "POST",
    credentials: "include",
    headers: { "Content-Type": "application/json" },
    body,
    keepalive: true,
  }).catch(() => {
    // best-effort
  });
}
