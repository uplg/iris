import { describe, expect, it } from 'vitest';
import { initSegmentEnd, initSegmentSplit, topLevelBoxes } from './fmp4';

function box(type: string, payload = 4): Uint8Array {
	const out = new Uint8Array(8 + payload);
	new DataView(out.buffer).setUint32(0, out.length);
	for (let i = 0; i < 4; i += 1) out[4 + i] = type.charCodeAt(i);
	return out;
}

function concat(...parts: Uint8Array[]): Uint8Array {
	const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
	let at = 0;
	for (const p of parts) {
		out.set(p, at);
		at += p.length;
	}
	return out;
}

describe('fMP4 boxes', () => {
	const ftyp = box('ftyp');
	const moov = box('moov', 20);
	const moof = box('moof', 8);
	const mdat = box('mdat', 100);

	it('walks the complete top-level boxes and stops at a truncated one', () => {
		const buf = concat(ftyp, moov, moof.subarray(0, 10));
		expect(topLevelBoxes(buf).map((b) => b.type)).toEqual(['ftyp', 'moov']);
	});

	it('splits an init segment glued to the first media segment', () => {
		const buf = concat(ftyp, moov, moof, mdat);
		const split = initSegmentSplit(buf);
		expect(split?.[0].length).toBe(ftyp.length + moov.length);
		expect(split?.[1].length).toBe(moof.length + mdat.length);
	});

	it('leaves media segments and lone init segments whole', () => {
		expect(initSegmentSplit(concat(moof, mdat))).toBeNull();
		expect(initSegmentSplit(concat(ftyp, moov))).toBeNull();
	});

	it('finds where moov ends, once it is complete', () => {
		expect(initSegmentEnd(ftyp)).toBe(-1);
		expect(initSegmentEnd(concat(ftyp, moov.subarray(0, 12)))).toBe(-1);
		expect(initSegmentEnd(concat(ftyp, moov, moof))).toBe(ftyp.length + moov.length);
	});
});
