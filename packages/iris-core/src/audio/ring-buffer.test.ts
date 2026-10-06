import { describe, expect, it } from 'vitest';
import { createRingBuffer, type PcmSource } from './ring-buffer';

const pcm = (planes: number[][]): PcmSource => ({
	numberOfFrames: planes[0]!.length,
	numberOfChannels: planes.length,
	copyTo: (dest, { planeIndex }) => dest.set(planes[planeIndex]!)
});

describe('ring buffer', () => {
	it('writes stereo whatever the source layout', () => {
		const ring = createRingBuffer(8);
		expect(ring.channels).toBe(2);
		expect(ring.push(pcm([[0.5, 0.5, 0.5]]))).toBe(3);
		expect(ring.bufferedFrames()).toBe(3);
	});

	it('never overwrites what the worklet has not played: it keeps what fits', () => {
		const ring = createRingBuffer(8);
		// one frame stays free to tell full from empty
		expect(ring.push(pcm([Array.from({ length: 20 }, () => 0.1), Array.from({ length: 20 }, () => 0.1)]))).toBe(7);
		expect(ring.push(pcm([[0.1], [0.1]]))).toBe(0);
		expect(ring.free()).toBe(0);
	});

	it('starts empty again on reset', () => {
		const ring = createRingBuffer(8);
		ring.push(pcm([[1, 1]]));
		ring.reset();
		expect(ring.bufferedFrames()).toBe(0);
		expect(ring.readIndex()).toBe(0);
	});
});
