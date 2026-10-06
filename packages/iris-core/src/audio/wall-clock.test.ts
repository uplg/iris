import { describe, expect, it } from 'vitest';
import { WallClock } from './wall-clock';

describe('WallClock', () => {
	it('reads 0 until anchored, then advances with the wall clock', () => {
		let now = 1_000;
		const clock = new WallClock(() => now);
		expect(clock.anchored).toBe(false);
		expect(clock.time()).toBe(0);
		clock.anchor(600);
		now += 2_500;
		expect(clock.time()).toBeCloseTo(602.5);
	});

	it('freezes while paused and goes on from there', () => {
		let now = 0;
		const clock = new WallClock(() => now);
		clock.anchor(10);
		now = 1_000;
		clock.pause();
		now = 60_000;
		expect(clock.time()).toBeCloseTo(11);
		clock.resume();
		now = 61_000;
		expect(clock.time()).toBeCloseTo(12);
	});

	it('anchored while paused, stays at the anchor until resumed', () => {
		let now = 0;
		const clock = new WallClock(() => now);
		clock.pause();
		clock.anchor(300);
		now = 5_000;
		expect(clock.time()).toBe(300);
		clock.resume();
		now = 6_000;
		expect(clock.time()).toBeCloseTo(301);
	});

	it('forgets its anchor on reset (a seek)', () => {
		const clock = new WallClock(() => 0);
		clock.anchor(5);
		clock.reset();
		expect(clock.anchored).toBe(false);
	});
});
