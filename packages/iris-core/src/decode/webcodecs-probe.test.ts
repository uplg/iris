import { describe, expect, it } from 'vitest';
import { recordFailure, skipsProbe } from './webcodecs-probe';

const HOUR = 60 * 60 * 1000;

describe('WebCodecs probe failure memory', () => {
	it('still tests a codec after a single failure', () => {
		const once = recordFailure(null, 0);
		expect(skipsProbe(once, HOUR)).toBe(false);
	});

	it('skips the test after two failures, for a day', () => {
		const twice = recordFailure(recordFailure(null, 0), HOUR);
		expect(skipsProbe(twice, 2 * HOUR)).toBe(true);
		expect(skipsProbe(twice, 26 * HOUR)).toBe(false);
	});

	it('starts the count over once the record expired', () => {
		const old = recordFailure(recordFailure(null, 0), 0);
		const again = recordFailure(old, 30 * HOUR);
		expect(skipsProbe(again, 30 * HOUR)).toBe(false);
	});

	it('ignores what it cannot read (the old permanent "fail" marker included)', () => {
		expect(skipsProbe('fail', 0)).toBe(false);
		expect(skipsProbe(null, 0)).toBe(false);
		expect(JSON.parse(recordFailure('fail', 5))).toEqual({ fails: 1, at: 5 });
	});
});
