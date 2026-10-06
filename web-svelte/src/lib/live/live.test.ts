import { describe, expect, it } from 'vitest';
import { liveManifest, liveTier, LiveRotation, MIN_AUTO_ROTATIONS, programmeProgress, sourceCount } from './live.ts';

describe('live tv', () => {
	it('places now inside a programme', () => {
		const start = '2026-10-06T20:00:00Z';
		const stop = '2026-10-06T21:00:00Z';
		expect(programmeProgress(start, stop, Date.parse('2026-10-06T20:15:00Z'))).toBe(25);
		expect(programmeProgress(start, stop, Date.parse('2026-10-06T21:15:00Z'))).toBeNull();
		expect(programmeProgress('nope', stop)).toBeNull();
	});

	it('plays the tuner through WebCodecs when it can, everything else through Tier F', () => {
		expect(liveTier('tuner', true)).toBe('C');
		expect(liveTier('tuner', false)).toBe('F');
		expect(liveTier('vavoo', true)).toBe('F');
		expect(liveTier(null, true)).toBe('F');
	});

	it('budgets rotations on the channel’s real source count', () => {
		expect(sourceCount('4')).toBe(4);
		expect(sourceCount(null)).toBe(MIN_AUTO_ROTATIONS);
		expect(sourceCount('0')).toBe(MIN_AUTO_ROTATIONS);
	});

	it('retries the same source once, then rotates, then gives up', () => {
		const r = new LiveRotation();
		const steps = Array.from({ length: 8 }, () => r.next(3));
		expect(steps).toEqual(['retry', 'rotate', 'retry', 'rotate', 'retry', 'rotate', 'retry', 'give-up']);
		r.reset();
		expect([r.next(3), r.next(3)]).toEqual(['retry', 'rotate']);
	});

	it('stands in an endless, trackless manifest', () => {
		const m = liveManifest('TF1');
		expect(m.duration_s).toBeNull();
		expect([m.audio.length, m.subtitles.length, m.filename]).toEqual([0, 0, 'TF1']);
	});
});
