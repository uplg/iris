/**
 * Client capability probing. Builds the `Iris-Caps` header value the
 * server-side telemetry + decision layer consumes.
 *
 * See docs/SOTA_ARCHITECTURE.md §2.2 for the wire format.
 *
 * Phase 0 probes the lowest-cost set of MediaSource / WebCodecs / WebGPU
 * facts. Per-codec hardware-acceleration probing (which lies on Chrome
 * Linux for HEVC) is deferred to Phase 2b's real keyframe-decode test.
 */

export type ClientCaps = {
  containers: string[];
  videoDecoders: string[];
  audioDecoders: string[];
  subtitles: string[];
  hdr: string[];
  webcodecs: boolean;
  webgpu: boolean;
  mse: boolean;
  /** iOS Safari ManagedMediaSource — newer, power-aware variant of MSE. */
  mms: boolean;
  platform: string;
};

const VIDEO_PROBES: Array<{ name: string; mime: string }> = [
  // The codec strings here are "generic" enough that browsers answer
  // truthfully for the capability dimension we care about — they say
  // "yes I can play H.264" rather than getting hung up on a specific
  // profile/level we made up.
  { name: "h264", mime: 'video/mp4; codecs="avc1.42E01E"' },
  { name: "hevc", mime: 'video/mp4; codecs="hev1.1.6.L93.B0"' },
  { name: "av1", mime: 'video/mp4; codecs="av01.0.08M.08"' },
  { name: "vp9", mime: 'video/webm; codecs="vp09.00.10.08"' },
  { name: "vp8", mime: 'video/webm; codecs="vp8"' },
];

const AUDIO_PROBES: Array<{ name: string; mime: string }> = [
  { name: "aac", mime: 'audio/mp4; codecs="mp4a.40.2"' },
  { name: "mp3", mime: "audio/mpeg" },
  { name: "opus", mime: 'audio/mp4; codecs="opus"' },
  { name: "vorbis", mime: 'audio/webm; codecs="vorbis"' },
  { name: "flac", mime: 'audio/mp4; codecs="flac"' },
  { name: "ac3", mime: 'audio/mp4; codecs="ac-3"' },
  { name: "eac3", mime: 'audio/mp4; codecs="ec-3"' },
];

const CONTAINER_PROBES: Array<{ name: string; mime: string }> = [
  { name: "fmp4", mime: 'video/mp4; codecs="avc1.42E01E"' },
  { name: "webm", mime: 'video/webm; codecs="vp9"' },
  // No browser accepts raw MKV in MSE; we keep the entry as a placeholder
  // for the future demux-via-mediabunny path (Phase 2a).
];

let cached: ClientCaps | null = null;
let cachedHeader: string | null = null;

export async function probeCapabilities(): Promise<ClientCaps> {
  if (cached) return cached;
  const mse = typeof globalThis.MediaSource !== "undefined";
  const mms =
    typeof (globalThis as { ManagedMediaSource?: unknown }).ManagedMediaSource !== "undefined";
  const webcodecs = typeof (globalThis as { VideoDecoder?: unknown }).VideoDecoder !== "undefined";
  const webgpu = typeof (navigator as Navigator & { gpu?: unknown }).gpu !== "undefined";

  const containers = mse
    ? CONTAINER_PROBES.filter((p) => MediaSource.isTypeSupported(p.mime)).map((p) => p.name)
    : [];
  // Phase 0: just MSE-level probing. WebCodecs HW probing comes Phase 2b.
  const videoDecoders = mse
    ? VIDEO_PROBES.filter((p) => MediaSource.isTypeSupported(p.mime)).map((p) => p.name)
    : [];
  const audioDecoders = mse
    ? AUDIO_PROBES.filter((p) => MediaSource.isTypeSupported(p.mime)).map((p) => p.name)
    : [];

  // Subtitles: WebVTT is always native via <track>. ASS/PGS overlays land
  // Phase 2d (libass-wasm, libpgs-js) — we still advertise them so server
  // telemetry surfaces clients that ought to receive overlay-rendered subs.
  const subtitles = ["webvtt"];

  const hdr: string[] = [];
  if (mse && MediaSource.isTypeSupported('video/mp4; codecs="hev1.2.4.L153.B0"')) {
    // hev1 Main10 / L5.1 is the HDR10 capability marker; deeper detection
    // (PQ vs HLG, max_cll honouring) lands Phase 2c when WebGPU canvases
    // get configured for HDR.
    hdr.push("hdr10", "hlg");
  }

  cached = {
    containers,
    videoDecoders,
    audioDecoders,
    subtitles,
    hdr,
    webcodecs,
    webgpu,
    mse,
    mms,
    platform: detectPlatform(),
  };
  cachedHeader = null;
  return cached;
}

export function capsHeader(caps: ClientCaps): string {
  if (cached === caps && cachedHeader !== null) return cachedHeader;
  const parts: string[] = [];
  if (caps.containers.length) parts.push(`container=${caps.containers.join(",")}`);
  if (caps.videoDecoders.length) parts.push(`vdec=${caps.videoDecoders.join(",")}`);
  if (caps.audioDecoders.length) parts.push(`adec=${caps.audioDecoders.join(",")}`);
  if (caps.subtitles.length) parts.push(`subs=${caps.subtitles.join(",")}`);
  if (caps.hdr.length) parts.push(`hdr=${caps.hdr.join(",")}`);
  if (caps.webcodecs) parts.push("webcodecs=1");
  if (caps.webgpu) parts.push("webgpu=1");
  if (caps.mse) parts.push("mse=1");
  if (caps.mms) parts.push("mms=1");
  parts.push(`platform=${caps.platform}`);
  const header = parts.join("; ");
  if (cached === caps) cachedHeader = header;
  return header;
}

function detectPlatform(): string {
  const ua = navigator.userAgent;
  // Order matters: Edge UA contains "Chrome", Brave contains "Chrome", etc.
  // We just want a coarse bucket for telemetry; PII concerns are minimal
  // since the same string lives in User-Agent already.
  let browser = "unknown";
  if (/Edg\//.test(ua)) browser = "edge";
  else if (/Firefox\//.test(ua)) browser = "firefox";
  else if (/Chrome\//.test(ua)) browser = "chromium";
  else if (/Safari\//.test(ua)) browser = "safari";
  const versionMatch =
    ua.match(/Edg\/(\d+)/) ??
    ua.match(/Firefox\/(\d+)/) ??
    ua.match(/Chrome\/(\d+)/) ??
    ua.match(/Version\/(\d+).*Safari/);
  const version = versionMatch ? versionMatch[1] : "?";
  return `web-${browser}-${version}`;
}

/**
 * Coarse "is this a phone / tablet?" check used to gate the decode-tier
 * cascade. Mobile browsers run in tight per-tab memory budgets and the
 * OOM killer reaps the renderer with NO recoverable JS error — the tab
 * just shows Chrome's "Aw, Snap!" page. Because that's unrecoverable at
 * runtime (the demote cascade can't fire on a dead renderer), the only
 * defence is to never *select* a memory-heavy engine (WebCodecs canvas
 * decode, WASM transcode, client-side demux) on these devices in the
 * first place — see `pickTier`.
 *
 * Detection: UA mobile markers OR (touch-capable AND coarse pointer).
 * The second clause catches Android tablets / UAs that omit "Mobi".
 * iPadOS Safari masquerades as desktop, but its native `<video>` + HLS
 * paths (Tier A / F) are exactly the ones we keep, so a false negative
 * there is harmless.
 */
export function isMobileLike(): boolean {
  if (typeof navigator === "undefined") return false;
  const ua = navigator.userAgent;
  if (/Android|iPhone|iPad|iPod|Mobi|Windows Phone/i.test(ua)) return true;
  const touch = (navigator.maxTouchPoints ?? 0) > 0 || "ontouchstart" in (globalThis as object);
  const coarse = typeof matchMedia === "function" && matchMedia("(pointer: coarse)").matches;
  return touch && coarse;
}

/** Test-only hook to reset the memoised result. */
export function __resetCapsCacheForTests(): void {
  cached = null;
  cachedHeader = null;
}
