import { describe, expect, it } from 'vitest';
import { StampQueue } from './stamp-queue';

describe('StampQueue', () => {
	it('gives each chunk the timeline position of the frame it encodes', () => {
		const q = new StampQueue();
		q.push(1_000, 0);
		q.push(21_000, 0.02);
		expect(q.take(1_000)).toBe(0);
		expect(q.take(21_000)).toBe(0.02);
		expect(q.size).toBe(0);
	});

	it('skips the frames a realtime encoder dropped instead of shifting every later stamp', () => {
		const q = new StampQueue();
		q.push(1_000, 0);
		q.push(21_000, 0.02);
		q.push(41_000, 0.04);
		expect(q.take(1_000)).toBe(0);
		// the frame at 21 000 µs never came out
		expect(q.take(41_000)).toBe(0.04);
		expect(q.dropped).toBe(1);
		expect(q.size).toBe(0);
	});

	it('falls back to feed order when the encoder does not echo timestamps', () => {
		const q = new StampQueue();
		q.push(1_000, 0);
		q.push(21_000, 0.02);
		expect(q.take(0)).toBe(0);
		expect(q.unmatched).toBe(1);
		expect(q.take(0)).toBe(0.02);
		expect(q.take(0)).toBeNull();
	});
});
