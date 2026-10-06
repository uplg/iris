/**
 * Stereo weights for a decoded channel layout, in WebCodecs' channel order (the
 * WAVEFORMATEXTENSIBLE / ffmpeg native order: FL FR FC LFE BL BR SL SR). The output is stereo
 * whatever the source carries: keeping only the first two channels of a 5.1 track drops the
 * centre — the dialogue — and a mono track would play in the left ear only.
 *
 * ITU-R BS.775 coefficients (centre and surrounds at −3 dB, LFE dropped), normalised so a
 * full-scale sample on every channel can't clip.
 */

const H = Math.SQRT1_2;

/** Per output channel (left, right), the weight of each source channel. */
export type StereoWeights = [number[], number[]];

function normalised(left: number[], right: number[]): StereoWeights {
	const peak = Math.max(
		left.reduce((a, b) => a + b, 0),
		right.reduce((a, b) => a + b, 0)
	);
	const k = peak > 1 ? 1 / peak : 1;
	return [left.map((w) => w * k), right.map((w) => w * k)];
}

export function stereoWeights(channels: number): StereoWeights {
	switch (channels) {
		case 1:
			return [[1], [1]];
		case 2:
			return [
				[1, 0],
				[0, 1]
			];
		case 3: // FL FR FC
			return normalised([1, 0, H], [0, 1, H]);
		case 4: // FL FR BL BR
			return normalised([1, 0, H, 0], [0, 1, 0, H]);
		case 5: // FL FR FC BL BR
			return normalised([1, 0, H, H, 0], [0, 1, H, 0, H]);
		case 6: // FL FR FC LFE BL BR
			return normalised([1, 0, H, 0, H, 0], [0, 1, H, 0, 0, H]);
		case 7: // FL FR FC LFE BC SL SR
			return normalised([1, 0, H, 0, 0.5, H, 0], [0, 1, H, 0, 0.5, 0, H]);
		case 8: // FL FR FC LFE BL BR SL SR
			return normalised([1, 0, H, 0, H, 0, H, 0], [0, 1, H, 0, 0, H, 0, H]);
		default: {
			// unknown layout: the front pair as it is
			const left = Array.from({ length: channels }, (_, i) => (i === 0 ? 1 : 0));
			const right = Array.from({ length: channels }, (_, i) => (i === 1 ? 1 : 0));
			return [left, right];
		}
	}
}

/** Interleaves `frames` frames of `planes` (one per source channel) into stereo `out`,
 *  starting at `offset` (an even index, in interleaved samples) and wrapping at `out.length`. */
export function mixToStereo(planes: Float32Array[], frames: number, weights: StereoWeights, out: Float32Array, offset: number): void {
	const [wl, wr] = weights;
	const n = planes.length;
	const cap = out.length;
	let w = offset;
	for (let i = 0; i < frames; i += 1) {
		let l = 0;
		let r = 0;
		for (let c = 0; c < n; c += 1) {
			const s = planes[c]![i]!;
			l += s * wl[c]!;
			r += s * wr[c]!;
		}
		out[w] = l;
		out[w + 1] = r;
		w += 2;
		if (w >= cap) w -= cap;
	}
}
