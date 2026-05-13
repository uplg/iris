/**
 * `Iris-Caps`-aware manifest client.
 *
 * Wraps the server's `/api/torrents/{infohash}/files/{idx}/manifest.json`
 * endpoint and the matching capability declaration. See
 * docs/SOTA_ARCHITECTURE.md §2.1 for the wire format. The TS types here are
 * the dual of `iris-media::manifest::Manifest` in Rust.
 */

import { capsHeader, probeCapabilities } from "./caps";
import { cheapProbeVideoCodec } from "./decode/webcodecs-probe";

export type HdrKind = "none" | "hdr10" | "hdr10_plus" | "dovi" | "hlg";

export type ByteRange = { start: number; end: number };

export type DownloadStatus = {
  progress: number;
  ranges_complete: Array<[number, number]>;
  bytes_complete: number;
};

export type VideoTrack = {
  stream_idx: number;
  codec: string;
  codec_string: string | null;
  profile: string | null;
  level: number | null;
  bit_depth: number | null;
  width: number | null;
  height: number | null;
  fps_num: number | null;
  fps_den: number | null;
  hdr: HdrKind;
  color_primaries: string | null;
  color_transfer: string | null;
  color_matrix: string | null;
  max_cll: number | null;
  max_fall: number | null;
};

export type AudioTrack = {
  stream_idx: number;
  codec: string;
  codec_string: string | null;
  channels: number;
  channel_layout: string | null;
  sample_rate: number | null;
  bitrate: number | null;
  lang: string | null;
  title: string | null;
  default: boolean;
  forced: boolean;
  browser_native: boolean;
};

export type SubtitleTrack = {
  stream_idx: number;
  codec: string;
  lang: string | null;
  title: string | null;
  default: boolean;
  forced: boolean;
  text_based: boolean;
  extractable: boolean;
  url: string;
};

export type Chapter = { start_s: number; end_s: number; title: string | null };

export type Manifest = {
  schema_version: number;
  infohash: string;
  file_idx: number;
  filename: string;
  container: string;
  duration_s: number | null;
  size_bytes: number;
  moov_at_start: boolean | null;
  index_at_end: boolean;
  header_byte_range: ByteRange;
  tail_byte_range: ByteRange | null;
  download: DownloadStatus;
  video: VideoTrack[];
  audio: AudioTrack[];
  subtitles: SubtitleTrack[];
  chapters: Chapter[];
};

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
    const mime = v.codec_string
      ? `video/mp4; codecs="${v.codec_string}"`
      : `video/mp4`;
    return MediaSource.isTypeSupported(mime);
  });
  const audioNative = manifest.audio.every((a) => a.browser_native);

  // Tier A/B require the codec to be MSE-friendly AND the audio to be
  // browser-native (Phase 2b alpha doesn't transcode audio yet).
  if (codecsMse && audioNative) {
    const isMp4Family = /mp4|mov|m4v|isobmff/i.test(manifest.container);
    if (isMp4Family) return "A";
    const tierBContainers = /matroska|webm|avi|mpegts|quicktime|mov/i;
    if (tierBContainers.test(manifest.container)) return "B";
  }

  // Tier C/D: codec MSE refuses but WebCodecs might decode it. Walk the
  // primary video track; if its codec_string passes the cheap
  // isConfigSupported check, we're a candidate. The full 1-frame test
  // runs at mount time so we don't pay it on every manifest.json fetch.
  const primary = manifest.video[0];
  if (primary?.codec_string) {
    const probe = await cheapProbeVideoCodec(primary.codec_string);
    if (probe.supportedHardware) return "C";
    if (probe.supportedAny) return "D";
  }

  // Tier E: HEVC at ≤ 1080p in a Chromium-family browser where neither
  // MSE nor WebCodecs accept the codec. hevc.js transcodes to H.264 in
  // a WASM worker. 4K is excluded because hevc.js hits ~21 fps on 4K.
  if (
    primary &&
    /hevc|hev1|hvc1|h265|x265/i.test(primary.codec) &&
    (primary.height ?? 0) <= 1080
  ) {
    const ua = navigator.userAgent;
    const chromiumish = /Chrome|Edg/.test(ua) && !/Mobile/.test(ua);
    if (chromiumish) return "E";
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
export function postSeekHint(
  manifest: Manifest,
  playheadSeconds: number,
): void {
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
