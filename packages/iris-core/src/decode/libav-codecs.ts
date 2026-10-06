// Kept apart from `libav-audio-decoder.ts`: `pickTier` asks it before any engine loads, and
// that module pulls mediabunny into the page's first chunk.

import type { AudioCodec } from 'mediabunny';

/**
 * Audio codecs handled by the libav-backed decoder.
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
const SUPPORTED: ReadonlySet<string> = new Set<AudioCodec>([
	'ac3',
	'eac3',
	'flac',
	// mediabunny surfaces `A_DTS` Matroska tracks natively since 1.55
	// (it used to need a local patch); the libav `dca` decoder picks up
	// the packets and produces PCM samples. DTS-HD MA core layer is
	// decoded; the extension substream is dropped (fine — Tier B
	// re-encodes to AAC anyway).
	'dts',
	'pcm-s16',
	'pcm-s24',
	'pcm-s32',
	'pcm-f32'
]);

export function libavCanDecode(codec: string): boolean {
	return SUPPORTED.has(codec);
}
