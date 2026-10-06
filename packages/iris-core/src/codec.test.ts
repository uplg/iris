import { describe, expect, it } from 'vitest';
import { isHevc } from './codec';

describe('isHevc', () => {
	it('knows every spelling of HEVC', () => {
		for (const c of ['hevc', 'HEVC', 'hev1.1.6.L120.90', 'hvc1.2.4.L153.B0', 'h265', 'x265']) expect(isHevc(c)).toBe(true);
	});

	it('says no to the other codecs and to nothing', () => {
		for (const c of ['h264', 'avc1.640028', 'av01.0.08M.08', 'vp09.00.10.08', '', null, undefined]) expect(isHevc(c)).toBe(false);
	});
});
