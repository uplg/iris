import { describe, expect, it } from 'vitest';
import { completeHvcc, escapeRbsp, HevcSpliceUnsupported, NEEDS_WHOLE_NAL, patchNalPrefix, unescapeRbsp } from './hevc-cra-splice';

/** Deterministic pseudo-random bytes, zero-heavy so emulation prevention happens a lot. */
function rbspBytes(seed: number, length: number): Uint8Array {
	let s = seed;
	const out = new Uint8Array(length);
	for (let i = 0; i < length; i += 1) {
		s = (s * 1103515245 + 12345) >>> 0;
		const r = s >>> 16;
		out[i] = r % 3 === 0 ? 0 : r % 5 === 0 ? (r >> 3) % 4 : r & 0xff;
	}
	// a NAL header never holds zeros, and the RBSP ends on its stop bit
	out[0] = 0x02;
	out[1] = 0x01;
	out[length - 1] = 0x80;
	return out;
}

const nalOf = (rbsp: Uint8Array) => {
	const escaped = escapeRbsp(rbsp.subarray(2));
	const nal = new Uint8Array(2 + escaped.length);
	nal.set(rbsp.subarray(0, 2));
	nal.set(escaped, 2);
	return nal;
};

/** Sets bits [from, from + n) of `rbsp` to `value`'s bits; returns the end byte. */
const writeBits = (rbsp: Uint8Array, from: number, n: number, value: number) => {
	for (let i = 0; i < n; i += 1) {
		const pos = from + i;
		const mask = 0x80 >> (pos & 7);
		const bit = (value >>> (n - 1 - i)) & 1;
		rbsp[pos >> 3] = bit ? rbsp[pos >> 3]! | mask : rbsp[pos >> 3]! & ~mask;
	}
	return Math.ceil((from + n) / 8);
};

describe('escapeRbsp', () => {
	it('inserts an emulation byte wherever 00 00 0x (x ≤ 3) would appear, and unescape undoes it', () => {
		const rbsp = Uint8Array.from([0, 0, 1, 0, 0, 0, 0, 0, 3, 5]);
		const escaped = escapeRbsp(rbsp);
		expect(Array.from(escaped)).toEqual([0, 0, 3, 1, 0, 0, 3, 0, 0, 3, 0, 3, 5]);
		expect(Array.from(unescapeRbsp(escaped))).toEqual(Array.from(rbsp));
	});
});

describe('patchNalPrefix', () => {
	it('rewrites a header field exactly as a whole-NAL unescape, patch and escape would', () => {
		let fastPaths = 0;
		for (let seed = 1; seed <= 300; seed += 1) {
			const rbsp = rbspBytes(seed, 400);
			const nal = nalOf(rbsp);
			const from = 16 + (seed % 120);
			const bits = 4 + (seed % 13);
			const value = (seed * 2654435761) >>> (32 - bits);

			const whole = unescapeRbsp(nal).slice();
			writeBits(whole, from, bits, value);
			const expected = nalOf(whole);

			const fast = patchNalPrefix(nal, 96, (head) => writeBits(head, from, bits, value));
			if (fast === NEEDS_WHOLE_NAL) continue;
			fastPaths += 1;
			expect(fast, `seed ${seed}`).toEqual(expected);
		}
		expect(fastPaths).toBeGreaterThan(250);
	});

	it('leaves the NAL alone when the patch changes nothing', () => {
		const nal = nalOf(rbspBytes(7, 200));
		expect(patchNalPrefix(nal, 96, () => null)).toBeNull();
	});

	it('asks for the whole NAL when the header runs past its window', () => {
		const nal = nalOf(rbspBytes(9, 300));
		const short = () => {
			throw new HevcSpliceUnsupported('slice header runs past the NAL unit');
		};
		expect(patchNalPrefix(nal, 8, short)).toBe(NEEDS_WHOLE_NAL);
		// anything else is a real failure
		expect(() =>
			patchNalPrefix(nal, 8, () => {
				throw new Error('broken');
			})
		).toThrow('broken');
	});
});

describe('completeHvcc', () => {
	// NAL headers: type << 1, then nuh_temporal_id_plus1 = 1
	const vps = Uint8Array.of(32 << 1, 1, 0xaa, 0xbb);
	const sps = Uint8Array.of(33 << 1, 1, 0xcc);
	const pps = Uint8Array.of(34 << 1, 1, 0xdd);
	const slice = Uint8Array.of(21 << 1, 1, 0xee, 0xff);
	const lengthPrefixed = (...nals: Uint8Array[]) => {
		const out: number[] = [];
		for (const nal of nals) out.push(0, 0, nal.length >> 8, nal.length & 0xff, ...nal);
		return Uint8Array.from(out);
	};
	const bare = () => {
		const h = new Uint8Array(23);
		h[0] = 1;
		h[21] = 0xff; // lengthSizeMinusOne = 3
		return h;
	};

	it('fills a bare record from the parameter sets the keyframe carries in-band', () => {
		const hvcc = bare();
		const out = completeHvcc(hvcc, lengthPrefixed(vps, sps, pps, slice));
		expect(Array.from(out.subarray(0, 22))).toEqual(Array.from(hvcc.subarray(0, 22)));
		expect(out[22]).toBe(3);
		expect(Array.from(out.subarray(23))).toEqual([
			0x80 | 32,
			0,
			1,
			0,
			4,
			...vps,
			0x80 | 33,
			0,
			1,
			0,
			3,
			...sps,
			0x80 | 34,
			0,
			1,
			0,
			3,
			...pps
		]);
	});

	it('leaves a record that has its own SPS and PPS alone', () => {
		const full = completeHvcc(bare(), lengthPrefixed(vps, sps, pps, slice));
		expect(completeHvcc(full, lengthPrefixed(Uint8Array.of(33 << 1, 1, 0x11), pps, slice))).toBe(full);
	});

	it('gives the record back unchanged when the keyframe has nothing to offer or does not parse', () => {
		const hvcc = bare();
		expect(completeHvcc(hvcc, lengthPrefixed(slice))).toBe(hvcc);
		expect(completeHvcc(hvcc, Uint8Array.of(0, 0, 0, 99, 1, 2))).toBe(hvcc);
		expect(completeHvcc(hvcc.subarray(0, 10), lengthPrefixed(vps, sps, pps))).toEqual(hvcc.subarray(0, 10));
	});
});
