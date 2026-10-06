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
 *   3. Failures are remembered in localStorage so later plays can
 *      short-circuit a codec this browser can't decode — but only once
 *      it failed twice, and only for a day: one failure may be a busy
 *      machine or a decoder pool the previous tier still held, and a
 *      timeout is never remembered at all. Positive results re-test
 *      every mount because the config object can't be cached safely.
 */

import type { InputVideoTrack } from 'mediabunny';
import { EncodedPacketSink } from 'mediabunny';

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

const CACHE_PREFIX = 'iris-core.wc-probe.v3.';
/** Failures before a codec is skipped without a test. */
const FAILS_TO_SKIP = 2;
/** How long a remembered failure holds. */
const FAIL_TTL_MS = 24 * 60 * 60 * 1000;

type FailEntry = { fails: number; at: number };

function parseEntry(raw: string | null): FailEntry | null {
	if (!raw) return null;
	try {
		const v = JSON.parse(raw) as Partial<FailEntry>;
		return typeof v.fails === 'number' && typeof v.at === 'number' ? { fails: v.fails, at: v.at } : null;
	} catch {
		return null;
	}
}

/** Whether a stored failure record says to skip the test (pure, for the tests). */
export function skipsProbe(raw: string | null, now: number): boolean {
	const e = parseEntry(raw);
	return !!e && e.fails >= FAILS_TO_SKIP && now - e.at < FAIL_TTL_MS;
}

/** The record after one more failure (an expired one starts over). */
export function recordFailure(raw: string | null, now: number): string {
	const e = parseEntry(raw);
	const fails = e && now - e.at < FAIL_TTL_MS ? e.fails + 1 : 1;
	return JSON.stringify({ fails, at: now } satisfies FailEntry);
}

function cacheKey(codec: string): string {
	const ua = navigator.userAgent.replace(/[^A-Za-z0-9]/g, '_').slice(0, 64);
	return `${CACHE_PREFIX}${codec}::${ua}`;
}

function readNegativeCache(codec: string): boolean {
	try {
		return skipsProbe(localStorage.getItem(cacheKey(codec)), Date.now());
	} catch {
		return false;
	}
}

function writeNegativeCache(codec: string): void {
	try {
		const key = cacheKey(codec);
		localStorage.setItem(key, recordFailure(localStorage.getItem(key), Date.now()));
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

type DecodeOutcome = 'decodes' | 'fails' | 'timeout';

/**
 * Real-decode test for a given config: `decodes` iff a `VideoFrame`
 * actually comes out within 5s of configure+decode. The decoder is
 * closed whatever happens (it holds a hardware slot until GC
 * otherwise). Logs the failing config to `console.debug` so the
 * developer can inspect the bytes that broke.
 */
async function realDecodeTest(config: VideoDecoderConfig, keyPacketChunk: EncodedVideoChunk): Promise<DecodeOutcome> {
	return new Promise<DecodeOutcome>((resolve) => {
		let settled = false;
		const timer = setTimeout(() => settle('timeout'), 5000);
		const settle = (outcome: DecodeOutcome) => {
			if (settled) return;
			settled = true;
			clearTimeout(timer);
			try {
				decoder.close();
			} catch {
				/* idempotent */
			}
			resolve(outcome);
		};
		const decoder = new VideoDecoder({
			output: (frame) => {
				frame.close();
				settle('decodes');
			},
			error: (err) => {
				if (!settled) console.debug('[iris-core] probe decode error', err, 'config:', summariseConfig(config));
				settle('fails');
			}
		});
		try {
			decoder.configure(config);
			decoder.decode(keyPacketChunk);
			void decoder.flush().catch(() => {
				/* error handler covers it */
			});
		} catch (e) {
			if (!settled) console.debug('[iris-core] probe configure threw', e, 'config:', summariseConfig(config));
			settle('fails');
		}
	});
}

function summariseConfig(c: VideoDecoderConfig): Record<string, unknown> {
	return {
		codec: c.codec,
		hardwareAcceleration: c.hardwareAcceleration,
		codedWidth: c.codedWidth,
		codedHeight: c.codedHeight,
		descriptionBytes:
			c.description instanceof ArrayBuffer ? c.description.byteLength : (c.description as ArrayBufferView | undefined)?.byteLength
	};
}

/** Materialise a `BufferSource` (ArrayBuffer / ArrayBufferView /
 *  SharedArrayBuffer) as a regular Uint8Array. Stored once, then
 *  `freshBuffer(bytes)` produces a brand-new ArrayBuffer copy each
 *  time we need to hand the description to a WebCodecs decoder. */
function bufferSourceToBytes(src: AllowSharedBufferSource): Uint8Array {
	if (src instanceof ArrayBuffer) return new Uint8Array(src).slice();
	if (typeof SharedArrayBuffer !== 'undefined' && src instanceof SharedArrayBuffer) {
		return new Uint8Array(new Uint8Array(src)).slice();
	}
	const view = src as ArrayBufferView;
	return new Uint8Array(view.buffer as ArrayBuffer, view.byteOffset, view.byteLength).slice();
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
export function configWithFreshDescription<T extends { description?: AllowSharedBufferSource | undefined }>(config: T): T {
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
export async function probeVideoTrack(track: InputVideoTrack): Promise<WebCodecsProbeResult | null> {
	if (typeof globalThis.VideoDecoder === 'undefined') return null;

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
	const descriptionBytes: Uint8Array | null = rawConfig.description ? bufferSourceToBytes(rawConfig.description) : null;
	const baseConfig: VideoDecoderConfig = {
		...rawConfig,
		description: descriptionBytes ? freshBuffer(descriptionBytes) : undefined
	};

	// Negative cache short-circuit: don't waste time on codecs we
	// already know fail in this browser. Positive cases re-test
	// because the config object can't be cached safely.
	if (readNegativeCache(baseConfig.codec)) {
		return {
			decodes: false,
			hardware: false,
			config: baseConfig,
			codec: baseConfig.codec
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
			codec: baseConfig.codec
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
		{ hwAcc: 'prefer-hardware', hardware: true },
		{ hwAcc: 'prefer-software', hardware: false },
		{ hwAcc: undefined, hardware: false }
	];
	let timedOut = false;
	for (const { hwAcc, hardware } of attempts) {
		const tryConfig: VideoDecoderConfig = {
			...baseConfig,
			...(hwAcc ? { hardwareAcceleration: hwAcc } : {}),
			description: descriptionBytes ? freshBuffer(descriptionBytes) : undefined
		};
		const support = await VideoDecoder.isConfigSupported(tryConfig).catch(() => ({ supported: false }) as VideoDecoderSupport);
		if (!support.supported) continue;
		const outcome = await realDecodeTest(tryConfig, chunk);
		if (outcome === 'timeout') timedOut = true;
		if (outcome === 'decodes') {
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
					description: descriptionBytes ? freshBuffer(descriptionBytes) : undefined
				},
				codec: baseConfig.codec
			};
		}
	}

	// No path produced a frame. Remember it (unless it only ran out of
	// time: a busy machine, not a verdict on the codec).
	if (!timedOut) writeNegativeCache(baseConfig.codec);
	return {
		decodes: false,
		hardware: false,
		config: baseConfig,
		codec: baseConfig.codec
	};
}
