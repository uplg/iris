import { describe, expect, it } from 'vitest';
import { mixToStereo, stereoWeights } from './downmix';

const mix = (planes: number[][], offset = 0, cap = planes[0]!.length * 2) => {
	const out = new Float32Array(cap);
	mixToStereo(
		planes.map((p) => Float32Array.from(p)),
		planes[0]!.length,
		stereoWeights(planes.length),
		out,
		offset
	);
	return Array.from(out);
};

describe('downmix', () => {
	it('plays mono in both ears', () => {
		expect(mix([[0.5, -0.25]])).toEqual([0.5, 0.5, -0.25, -0.25]);
	});

	it('passes stereo through', () => {
		expect(
			mix([
				[0.1, 0.2],
				[0.3, 0.4]
			])
		).toEqual([0.1, 0.3, 0.2, 0.4].map(Math.fround));
	});

	it('keeps the 5.1 centre (dialogue) in both channels and drops the LFE', () => {
		// FL FR FC LFE BL BR: only the centre speaks
		const [l, r] = mix([[0], [0], [1], [1], [0], [0]]);
		expect(l).toBeGreaterThan(0.25);
		expect(l).toBeCloseTo(r!);
		const [lfeOnly] = mix([[0], [0], [0], [1], [0], [0]]);
		expect(lfeOnly).toBe(0);
	});

	it('never clips a full-scale 5.1 or 7.1 frame', () => {
		for (const n of [6, 8]) {
			const [l, r] = mix(Array.from({ length: n }, () => [1]));
			expect(l).toBeLessThanOrEqual(1);
			expect(r).toBeLessThanOrEqual(1);
		}
	});

	it('wraps at the end of the ring', () => {
		const out = mix([[1, 2]], 2, 4);
		expect(out).toEqual([2, 2, 1, 1]);
	});
});
