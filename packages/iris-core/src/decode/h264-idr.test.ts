import { describe, expect, it } from 'vitest';
import { nalLengthSize, packetHasIdr } from './h264-idr.ts';

const AUD = [0x09, 0xf0];
const SEI = [0x06, 0x05, 0x01, 0x00];
const IDR = [0x65, 0x88, 0x84];
const NON_IDR_I = [0x41, 0x9a, 0x02];

const annexB = (...nals: number[][]) => new Uint8Array(nals.flatMap((n, i) => [...(i === 0 ? [0, 0, 0, 1] : [0, 0, 1]), ...n]));
const avcc = (...nals: number[][]) => new Uint8Array(nals.flatMap((n) => [0, 0, 0, n.length, ...n]));

describe('the live IDR anchor', () => {
	it('reads the NAL length size from the avcC, Annex B without one', () => {
		expect(nalLengthSize(undefined)).toBe(0);
		expect(nalLengthSize(new Uint8Array([1, 0x64, 0, 0x28, 0xff]))).toBe(4);
		expect(nalLengthSize(new Uint8Array([1, 0x64, 0, 0x28, 0xfd]))).toBe(2);
	});

	it('finds the IDR in length-prefixed samples (MP4)', () => {
		expect(packetHasIdr(avcc(AUD, SEI, IDR), 4)).toBe(true);
		expect(packetHasIdr(avcc(AUD, SEI, NON_IDR_I), 4)).toBe(false);
	});

	it('finds the IDR in Annex B samples (MPEG-TS)', () => {
		expect(packetHasIdr(annexB(AUD, SEI, IDR), 0)).toBe(true);
		expect(packetHasIdr(annexB(AUD, SEI, NON_IDR_I), 0)).toBe(false);
		// read as length-prefixed, an Annex B IDR was never found: the bench's live ?tier=B hunt
		expect(packetHasIdr(annexB(AUD, SEI, IDR), 4)).toBe(false);
	});
});
