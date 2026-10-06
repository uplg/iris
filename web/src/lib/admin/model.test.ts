import { describe, expect, it } from 'vitest';
import type { ActiveSession } from '@iris/api/client';
import { byDay, isView, liveState, olderThan } from './model.ts';

describe('admin lists by day', () => {
	const now = new Date(2026, 9, 6, 15, 0).getTime();
	const at = (d: number, h: number) => ({ at: new Date(2026, 9, d, h).toISOString() });

	it('each run of one day under its heading, in the order given', () => {
		const days = byDay([at(6, 14), at(6, 9), at(5, 22), at(3, 12)], (x) => x.at, now);
		expect(days.map((d) => [d.heading, d.items.length])).toEqual([
			['Today', 2],
			['Yesterday', 1],
			['Saturday 3 October', 1]
		]);
		expect(byDay([], (x: { at: string }) => x.at, now)).toEqual([]);
	});

	it('a view is one of the four', () => {
		expect(isView('people')).toBe(true);
		expect(isView('everything')).toBe(false);
		expect(isView(null)).toBe(false);
	});
});

describe('live sessions', () => {
	it('a version older than the release', () => {
		expect(olderThan('1.4.2', '1.5.0')).toBe(true);
		expect(olderThan('1.5.0', '1.5.0')).toBe(false);
		expect(olderThan('1.10.0', '1.9.3')).toBe(false);
		expect(olderThan('2.0.0-rc1', '1.9.0')).toBe(false);
		expect(olderThan(null, '1.5.0')).toBe(false);
	});

	it('playing, paused, buffering, or stalled once silent', () => {
		const now = Date.now();
		const s = (state: string, silentS: number) => ({ state, last_seen_at: new Date(now - silentS * 1000).toISOString() }) as ActiveSession;
		expect(liveState(s('playing', 5), now)).toBe('playing');
		expect(liveState(s('buffering', 5), now)).toBe('buffering');
		expect(liveState(s('paused', 40), now)).toBe('paused');
		expect(liveState(s('playing', 30), now)).toBe('stalled');
		expect(liveState(s('buffering', 30), now)).toBe('stalled');
	});
});
