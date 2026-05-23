/**
 * libav.js-backed `CustomAudioDecoder` for Mediabunny.
 *
 * Plugs into Mediabunny's `registerDecoder` so that any compressed
 * audio codec WebCodecs refuses (`ac3`, `eac3`, `flac` today) is
 * decoded to PCM via libav.js → Mediabunny re-encodes to AAC →
 * fragmented MP4 → MSE. The whole transcode runs in the browser.
 *
 * Tier B picks this path automatically when:
 *   1. The manifest's audio codec isn't `browser_native` (i.e., not
 *      AAC/Opus/MP3/Vorbis) AND
 *   2. The codec IS in our libav-supported set (see `SUPPORTED`).
 *
 * Mediabunny calls `LibavAudioDecoder.supports()` to decide whether
 * to use us. Returning `true` for an unsupported codec inside libav
 * would crash mid-decode, so the gate is conservative.
 */

import {
  AudioSample,
  CustomAudioDecoder,
  registerDecoder,
  type AudioCodec,
  type EncodedPacket,
} from "mediabunny";

/**
 * Audio codecs handled by this libav-backed decoder.
 *
 * The Iris variant of libav.js (built in the Dockerfile's
 * `libav-builder` stage) bundles `ac3`, `eac3`, `flac`, plus all
 * PCM flavours we care about. The npm-shipped `default` variant
 * is a strict subset of this (FLAC + PCM only) — when running
 * outside Docker (dev), the `iris.wasm.*` files don't exist and
 * libav falls back to `default`, in which case `ac3`/`eac3`
 * `ff_init_decoder` returns "Codec not found" and we surface a
 * Tier B mount error that the IrisPlayer demotes to F.
 */
const SUPPORTED: ReadonlySet<AudioCodec> = new Set<AudioCodec>([
  "ac3",
  "eac3",
  "flac",
  // `dts` is wired through a local mediabunny patch (see
  // `patches/mediabunny+*.patch`) — vanilla upstream's Matroska
  // demuxer skips `A_DTS` tracks because it doesn't carry the
  // codec ID in its map. Our patch adds the mapping; the libav
  // `dca` decoder picks up the packets and produces PCM samples.
  // DTS-HD MA core layer is decoded; the extension substream is
  // dropped (fine — Tier B re-encodes to AAC anyway).
  "dts",
  "pcm-s16",
  "pcm-s24",
  "pcm-s32",
  "pcm-f32",
]);

export function libavCanDecode(codec: string): boolean {
  return SUPPORTED.has(codec as AudioCodec);
}

// AV_SAMPLE_FMT_* values from libav. The number is the libav enum
// value as exported in the JS bindings.
const AV_SAMPLE_FMT_TO_MEDIABUNNY: Record<number, string> = {
  0: "u8",
  1: "s16",
  2: "s32",
  3: "f32",
  5: "u8-planar",
  6: "s16-planar",
  7: "s32-planar",
  8: "f32-planar",
};

// Concrete libav.js public surface we touch — typed loosely because
// libav.js doesn't ship TS types in any stable form.
type LibavLike = {
  ff_init_decoder: (
    name: string,
    config?: Record<string, unknown>,
  ) => Promise<[number, number, number, number]>;
  ff_decode_multi: (
    c: number,
    pkt: number,
    frame: number,
    packets: Array<{
      data: Uint8Array;
      pts: number;
      ptshi: number;
      dts: number;
      dtshi: number;
    }>,
    finalize?: boolean,
  ) => Promise<DecodedFrame[]>;
  ff_free_decoder: (c: number, pkt: number, frame: number) => Promise<void>;
  AVCodecContext_sample_rate_s: (c: number, v: number) => Promise<void>;
  AVCodecContext_channels_s: (c: number, v: number) => Promise<void>;
};

type DecodedFrame = {
  format: number;
  sample_rate: number;
  channels: number;
  nb_samples: number;
  pts: number;
  ptshi: number;
  /** Interleaved formats: a single TypedArray of samples.
   *  Planar formats: array of TypedArrays, one per channel. */
  data: ArrayBufferView | ArrayBufferView[];
};

let libavSingleton: Promise<LibavLike> | null = null;

/** Detect whether the Iris custom libav variant (with AC-3 / E-AC-3
 *  codecs) is deployed alongside the default variant. The build only
 *  ships it inside the Docker image (see `libav-builder` stage); dev
 *  servers running outside Docker fall back to `default`. */
async function detectIrisVariant(): Promise<boolean> {
  try {
    const res = await fetch("/libavjs/libav-6.8.8.0-iris.wasm.mjs", {
      method: "HEAD",
      cache: "no-store",
    });
    return res.ok;
  } catch {
    return false;
  }
}

function getLibav(): Promise<LibavLike> {
  if (libavSingleton) return libavSingleton;
  libavSingleton = (async () => {
    const variant = (await detectIrisVariant()) ? "iris" : "default";
    if (variant === "default") {
      console.warn(
        "[iris-core] Iris libav variant not found — falling back to `default` " +
          "(no AC-3 / E-AC-3 client-side decode). Run the libav-builder Docker " +
          "stage or `docker compose build` to produce the iris variant.",
      );
    }
    const mod = await import("libav.js");
    const factory =
      (mod as unknown as { LibAV?: (opts: object) => Promise<LibavLike> }).LibAV ??
      (mod as unknown as { default?: { LibAV?: (opts: object) => Promise<LibavLike> } }).default
        ?.LibAV;
    if (!factory) throw new Error("libav.js: LibAV factory not found");
    return factory({
      base: "/libavjs",
      // Run on the main thread for now. AudioWorklet-thread or
      // dedicated-Worker variants are a later polish — the audio
      // decode path runs at < 1 % CPU on a laptop so it's fine here.
      noworker: true,
      nothreads: true,
      variant,
    });
  })();
  return libavSingleton;
}

class LibavAudioDecoder extends CustomAudioDecoder {
  static supports(codec: AudioCodec, _config: AudioDecoderConfig): boolean {
    return SUPPORTED.has(codec);
  }

  private libav: LibavLike | null = null;
  private c = 0;
  private pkt = 0;
  private frame = 0;

  async init(): Promise<void> {
    this.libav = await getLibav();
    const name = mediabunnyToLibavCodecName(this.codec);
    const [ret, c, pkt, frame] = await this.libav.ff_init_decoder(name);
    if (ret < 0) {
      throw new Error(`libav: ff_init_decoder(${name}) returned ${ret}`);
    }
    this.c = c;
    this.pkt = pkt;
    this.frame = frame;
    if (this.config.sampleRate) {
      await this.libav.AVCodecContext_sample_rate_s(this.c, this.config.sampleRate);
    }
    if (this.config.numberOfChannels) {
      await this.libav.AVCodecContext_channels_s(this.c, this.config.numberOfChannels);
    }
  }

  async decode(packet: EncodedPacket): Promise<void> {
    if (!this.libav) return;
    const pts = Math.round(packet.timestamp * 1_000_000);
    const ptsLo = pts >>> 0;
    const ptsHi = Math.floor(pts / 4_294_967_296);
    const frames = await this.libav.ff_decode_multi(
      this.c,
      this.pkt,
      this.frame,
      [{ data: packet.data, pts: ptsLo, ptshi: ptsHi, dts: ptsLo, dtshi: ptsHi }],
      false,
    );
    for (const f of frames) {
      const sample = this.frameToAudioSample(f);
      if (sample) this.onSample(sample);
    }
  }

  async flush(): Promise<void> {
    if (!this.libav) return;
    const frames = await this.libav.ff_decode_multi(this.c, this.pkt, this.frame, [], true);
    for (const f of frames) {
      const sample = this.frameToAudioSample(f);
      if (sample) this.onSample(sample);
    }
  }

  async close(): Promise<void> {
    if (this.libav && this.c) {
      try {
        await this.libav.ff_free_decoder(this.c, this.pkt, this.frame);
      } catch {
        /* idempotent */
      }
      this.c = 0;
      this.pkt = 0;
      this.frame = 0;
    }
  }

  private frameToAudioSample(f: DecodedFrame): AudioSample | null {
    const format = AV_SAMPLE_FMT_TO_MEDIABUNNY[f.format];
    if (!format) {
      console.warn(`[iris-core] libav frame with unsupported format ${f.format}`);
      return null;
    }
    const isPlanar = format.endsWith("-planar");
    let bytes: Uint8Array;
    if (isPlanar && Array.isArray(f.data)) {
      // Planar: concatenate channel planes into one buffer in
      // channel-major order (the layout WebCodecs/Mediabunny expects).
      const planes = f.data;
      const total = planes.reduce((acc, p) => acc + p.byteLength, 0);
      bytes = new Uint8Array(total);
      let off = 0;
      for (const p of planes) {
        const view = new Uint8Array(p.buffer as ArrayBuffer, p.byteOffset, p.byteLength);
        bytes.set(view, off);
        off += view.byteLength;
      }
    } else {
      const view = (Array.isArray(f.data) ? f.data[0]! : f.data) as ArrayBufferView;
      bytes = new Uint8Array(view.buffer as ArrayBuffer, view.byteOffset, view.byteLength);
      bytes = bytes.slice();
    }
    const ptsMicro = (f.ptshi >>> 0) * 4_294_967_296 + (f.pts >>> 0);
    return new AudioSample({
      data: bytes.buffer,
      format: format as AudioSample["format"],
      numberOfChannels: f.channels,
      sampleRate: f.sample_rate,
      timestamp: ptsMicro / 1_000_000,
    });
  }
}

function mediabunnyToLibavCodecName(codec: AudioCodec): string {
  switch (codec) {
    case "ac3":
      return "ac3";
    case "eac3":
      return "eac3";
    case "flac":
      return "flac";
    // ffmpeg's DTS decoder is named `dca` (DTS Coherent Acoustics).
    case "dts":
      return "dca";
    case "pcm-s16":
      return "pcm_s16le";
    case "pcm-s24":
      return "pcm_s24le";
    case "pcm-s32":
      return "pcm_s32le";
    case "pcm-f32":
      return "pcm_f32le";
    default:
      return codec;
  }
}

let registered = false;
export function ensureLibavAudioDecoderRegistered(): void {
  if (registered) return;
  registered = true;
  // Mediabunny's `registerDecoder` accepts the decoder class itself.
  // We hide the cast behind a function so callers don't import
  // CustomAudioDecoder directly.
  registerDecoder(LibavAudioDecoder);
}
