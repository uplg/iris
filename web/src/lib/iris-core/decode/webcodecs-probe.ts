/**
 * Real WebCodecs capability probe — distinguishes hardware-decoded
 * "supported" from the false-positive `isConfigSupported` answers
 * browsers can give (Chrome Linux notoriously lies for HEVC, and
 * occasional Chrome macOS releases mis-canonicalise the config so
 * the returned `hw.config` rejects later in `configure()`).
 *
 * Strategy:
 *   1. Build a *fresh* base config from `track.getDecoderConfig()`.
 *      That gives us a live `description` BufferSource — it doesn't
 *      survive JSON roundtripping, so we never persist it.
 *   2. For each acceleration preference (`prefer-hardware`,
 *      `prefer-software`, none), call `isConfigSupported` and, if it
 *      says yes, **actually decode** a key packet. If a `VideoFrame`
 *      comes out, return that exact config — it's guaranteed to
 *      configure later.
 *   3. The decision (any-decodes / hardware) is cached in
 *      localStorage so subsequent plays can short-circuit a *failed*
 *      probe (no point retrying). Positive results re-test every
 *      mount because the config object can't be cached safely.
 */

import type { InputVideoTrack } from "mediabunny";
import { EncodedPacketSink } from "mediabunny";

export type WebCodecsProbeResult = {
  /** True iff a `VideoFrame` actually came out. */
  decodes: boolean;
  /** True when `prefer-hardware` was requested AND a frame decoded. */
  hardware: boolean;
  /** Fresh, ready-to-`configure()` config including the runtime
   *  `description` buffer. Never persisted between sessions. */
  config: VideoDecoderConfig;
  /** Diagnostic — the codec string we asked the browser about. */
  codec: string;
};

/** Cheap synchronous-ish probe used by `pickTier`. Just calls
 *  `VideoDecoder.isConfigSupported`. "Supported" here only means
 *  "worth trying"; the real test runs at mount time. */
export async function cheapProbeVideoCodec(codec: string): Promise<{
  supportedHardware: boolean;
  supportedAny: boolean;
}> {
  if (typeof globalThis.VideoDecoder === "undefined") {
    return { supportedHardware: false, supportedAny: false };
  }
  const baseConfig: VideoDecoderConfig = { codec };
  const hw = await VideoDecoder.isConfigSupported({
    ...baseConfig,
    hardwareAcceleration: "prefer-hardware",
  }).catch(() => ({ supported: false } as VideoDecoderSupport));
  if (hw.supported) return { supportedHardware: true, supportedAny: true };
  const sw = await VideoDecoder.isConfigSupported({
    ...baseConfig,
    hardwareAcceleration: "prefer-software",
  }).catch(() => ({ supported: false } as VideoDecoderSupport));
  return { supportedHardware: false, supportedAny: sw.supported ?? false };
}

const CACHE_PREFIX = "iris-core.wc-probe.v2.";

function cacheKey(codec: string): string {
  const ua = navigator.userAgent.replace(/[^A-Za-z0-9]/g, "_").slice(0, 64);
  return `${CACHE_PREFIX}${codec}::${ua}`;
}

function readNegativeCache(codec: string): boolean {
  try {
    return localStorage.getItem(cacheKey(codec)) === "fail";
  } catch {
    return false;
  }
}

function writeNegativeCache(codec: string): void {
  try {
    localStorage.setItem(cacheKey(codec), "fail");
  } catch {
    /* quota */
  }
}

function clearNegativeCache(codec: string): void {
  try {
    localStorage.removeItem(cacheKey(codec));
  } catch {
    /* idempotent */
  }
}

/**
 * Real-decode test for a given config. Resolves to true iff a
 * `VideoFrame` actually comes out within 5s of configure+decode.
 * Closes the decoder on the way out. Logs the failing config to
 * `console.debug` so the developer can inspect the bytes that broke.
 */
async function realDecodeTest(
  config: VideoDecoderConfig,
  keyPacketChunk: EncodedVideoChunk,
): Promise<boolean> {
  return new Promise<boolean>((resolve) => {
    let settled = false;
    const decoder = new VideoDecoder({
      output: (frame) => {
        frame.close();
        if (settled) return;
        settled = true;
        try {
          decoder.close();
        } catch {
          /* idempotent */
        }
        resolve(true);
      },
      error: (err) => {
        if (settled) return;
        settled = true;
        console.debug(
          "[iris-core] probe decode error",
          err,
          "config:",
          summariseConfig(config),
        );
        resolve(false);
      },
    });
    try {
      decoder.configure(config);
      decoder.decode(keyPacketChunk);
      void decoder.flush().catch(() => {
        /* error handler covers it */
      });
    } catch (e) {
      if (!settled) {
        settled = true;
        console.debug(
          "[iris-core] probe configure threw",
          e,
          "config:",
          summariseConfig(config),
        );
        resolve(false);
      }
    }
    setTimeout(() => {
      if (!settled) {
        settled = true;
        try {
          decoder.close();
        } catch {
          /* idempotent */
        }
        resolve(false);
      }
    }, 5000);
  });
}

function summariseConfig(c: VideoDecoderConfig): Record<string, unknown> {
  return {
    codec: c.codec,
    hardwareAcceleration: c.hardwareAcceleration,
    codedWidth: c.codedWidth,
    codedHeight: c.codedHeight,
    descriptionBytes:
      c.description instanceof ArrayBuffer
        ? c.description.byteLength
        : (c.description as ArrayBufferView | undefined)?.byteLength,
  };
}

/** Materialise a `BufferSource` (ArrayBuffer / ArrayBufferView /
 *  SharedArrayBuffer) as a regular Uint8Array. Stored once, then
 *  `freshBuffer(bytes)` produces a brand-new ArrayBuffer copy each
 *  time we need to hand the description to a WebCodecs decoder. */
function bufferSourceToBytes(src: AllowSharedBufferSource): Uint8Array {
  if (src instanceof ArrayBuffer) return new Uint8Array(src).slice();
  if (typeof SharedArrayBuffer !== "undefined" && src instanceof SharedArrayBuffer) {
    return new Uint8Array(new Uint8Array(src)).slice();
  }
  const view = src as ArrayBufferView;
  return new Uint8Array(
    view.buffer as ArrayBuffer,
    view.byteOffset,
    view.byteLength,
  ).slice();
}

/** Produce a brand-new owned `ArrayBuffer` containing the same bytes.
 *  Use before every `configure()` because some Chromium builds detach
 *  the buffer when handing it to the codec internals. */
export function freshBuffer(bytes: Uint8Array): ArrayBuffer {
  return bytes.slice().buffer;
}

/** Return a copy of `config` whose `description` field is a brand-new
 *  `ArrayBuffer`. Idempotent / cheap. Call right before any
 *  `VideoDecoder.configure()` / `AudioDecoder.configure()` so a config
 *  that previously flowed through a decoder (which may have detached
 *  its description in some Chromium builds) can be safely re-used. */
export function configWithFreshDescription<
  T extends { description?: AllowSharedBufferSource | undefined },
>(config: T): T {
  if (!config.description) return config;
  const bytes = bufferSourceToBytes(config.description);
  return { ...config, description: freshBuffer(bytes) };
}

/**
 * Probe a video track for WebCodecs decode support. Returns the
 * exact config that produced a `VideoFrame` (or null if none did).
 * The config returned is guaranteed to be `configure`-able when
 * passed back unchanged.
 */
export async function probeVideoTrack(
  track: InputVideoTrack,
): Promise<WebCodecsProbeResult | null> {
  if (typeof globalThis.VideoDecoder === "undefined") return null;

  const rawConfig = await track.getDecoderConfig();
  if (!rawConfig) return null;
  // The `description` BufferSource has to be re-cloned **before every
  // single `configure()` call** because some Chromium versions detach
  // the buffer when handing it to the codec internals (spec-non-
  // conforming: the spec says configure makes a copy). Sharing one
  // ArrayBuffer between probe-decoder and pipeline-decoder reproduces
  // the "Unsupported configuration" rejection on the second call.
  //
  // We keep the original bytes in `descriptionBytes` and stamp a fresh
  // copy onto the live config every time it leaves this module.
  const descriptionBytes: Uint8Array | null = rawConfig.description
    ? bufferSourceToBytes(rawConfig.description)
    : null;
  const baseConfig: VideoDecoderConfig = {
    ...rawConfig,
    description: descriptionBytes ? freshBuffer(descriptionBytes) : undefined,
  };

  // Negative cache short-circuit: don't waste time on codecs we
  // already know fail in this browser. Positive cases re-test
  // because the config object can't be cached safely.
  if (readNegativeCache(baseConfig.codec)) {
    return {
      decodes: false,
      hardware: false,
      config: baseConfig,
      codec: baseConfig.codec,
    };
  }

  // Pull the first key packet once and reuse across acceleration
  // attempts. Mediabunny opens it lazily.
  const sink = new EncodedPacketSink(track);
  const keyPacket = await sink.getFirstKeyPacket().catch(() => null);
  if (!keyPacket) {
    writeNegativeCache(baseConfig.codec);
    return {
      decodes: false,
      hardware: false,
      config: baseConfig,
      codec: baseConfig.codec,
    };
  }
  const chunk = keyPacket.toEncodedVideoChunk();

  // Try preferences in order. Note: we pass the raw shape
  // `{ ...baseConfig, hardwareAcceleration }` to `configure` rather
  // than the canonicalised `isConfigSupported.config` — some Chromium
  // versions return a `hw.config` whose `description` is internally-
  // referenced storage that fails to round-trip back to
  // `configure()`. Using the original baseConfig.description avoids
  // that footgun.
  const attempts: Array<{ hwAcc?: HardwareAcceleration; hardware: boolean }> = [
    { hwAcc: "prefer-hardware", hardware: true },
    { hwAcc: "prefer-software", hardware: false },
    { hwAcc: undefined, hardware: false },
  ];
  for (const { hwAcc, hardware } of attempts) {
    const tryConfig: VideoDecoderConfig = {
      ...baseConfig,
      ...(hwAcc ? { hardwareAcceleration: hwAcc } : {}),
      description: descriptionBytes ? freshBuffer(descriptionBytes) : undefined,
    };
    const support = await VideoDecoder.isConfigSupported(tryConfig).catch(
      () => ({ supported: false } as VideoDecoderSupport),
    );
    if (!support.supported) continue;
    const decodes = await realDecodeTest(tryConfig, chunk);
    if (decodes) {
      // Success — clear any stale negative entry from a previous
      // session and return a config with a *fresh* description so the
      // caller's `configure()` doesn't hit a buffer the test decoder
      // may have detached.
      clearNegativeCache(baseConfig.codec);
      return {
        decodes: true,
        hardware,
        config: {
          ...tryConfig,
          description: descriptionBytes ? freshBuffer(descriptionBytes) : undefined,
        },
        codec: baseConfig.codec,
      };
    }
  }

  // No path produced a frame. Remember it so future plays of the
  // same codec skip the test.
  writeNegativeCache(baseConfig.codec);
  return {
    decodes: false,
    hardware: false,
    config: baseConfig,
    codec: baseConfig.codec,
  };
}
