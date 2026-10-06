import { describe, expect, it } from 'vitest';
import {
	aheadInRange,
	bufferedAhead,
	bufferedSpan,
	coversTime,
	describeRanges,
	evictionSpan,
	forwardGapTarget,
	landsAt,
	ranges
} from './ranges';

describe('bufferedAhead', () => {
	it('bridges fragments that did not coalesce into one range', () => {
		const b = ranges([
			[0, 10],
			[10.4, 20],
			[21.5, 30]
		]);
		expect(bufferedAhead(b, 5)).toBe(25);
	});

	it('stops at a real hole', () => {
		const b = ranges([
			[0, 10],
			[14, 30]
		]);
		expect(bufferedAhead(b, 5)).toBe(5);
	});

	it('counts a range starting just past the playhead, and nothing when none covers it', () => {
		expect(bufferedAhead(ranges([[10.3, 20]]), 10)).toBeCloseTo(10);
		expect(bufferedAhead(ranges([[12, 20]]), 10)).toBe(0);
		expect(bufferedAhead(ranges([]), 10)).toBe(0);
	});
});

describe('coverage', () => {
	const b = ranges([
		[0, 10],
		[20, 30]
	]);

	it('says whether a time is buffered, with slack at both edges', () => {
		expect(coversTime(b, 10.2)).toBe(true);
		expect(coversTime(b, 15)).toBe(false);
		expect(coversTime(b, 19.8)).toBe(true);
	});

	it('lands a deferred playhead only where data follows it', () => {
		expect(landsAt(b, 19.8)).toBe(true);
		expect(landsAt(b, 10.2)).toBe(false);
	});

	it('measures what is left in the range holding the playhead', () => {
		expect(aheadInRange(b, 25)).toBe(5);
		expect(aheadInRange(b, 15)).toBe(0);
	});

	it('sums and describes the ranges', () => {
		expect(bufferedSpan(b)).toBe(20);
		expect(describeRanges(b)).toBe('0-10 20-30');
		expect(describeRanges(ranges([]))).toBe('empty');
	});
});

describe('evictionSpan', () => {
	const b = ranges([[100, 400]]);

	it('removes the played-out media beyond what is kept', () => {
		expect(evictionSpan(b, 200, 30)).toEqual([100, 170]);
	});

	it('waits until the span is worth an operation', () => {
		expect(evictionSpan(b, 132, 30, 5)).toBeNull();
		expect(evictionSpan(b, 136, 30, 5)).toEqual([100, 106]);
	});

	it('has nothing to do near the start or on an empty buffer', () => {
		expect(evictionSpan(b, 120, 30)).toBeNull();
		expect(evictionSpan(ranges([]), 200, 30)).toBeNull();
		expect(evictionSpan(b, 20, 30)).toBeNull();
	});
});

describe('forwardGapTarget', () => {
	it('jumps to the next range within reach, skipping zero-width leftovers', () => {
		const b = ranges([
			[0, 10],
			[10.5, 10.52],
			[13, 30]
		]);
		expect(forwardGapTarget(b, 10)).toBe(13);
	});

	it('does not jump across a hole wider than the reach', () => {
		expect(forwardGapTarget(ranges([[30, 60]]), 10)).toBeNull();
	});
});
