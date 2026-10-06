import { describe, expect, it } from 'vitest';
import { drawLoop, FrameQueue } from './frame-queue';

const frame = (seconds: number) => {
	const f = { timestamp: Math.round(seconds * 1_000_000), closed: false, close: () => (f.closed = true) };
	return f;
};

describe('FrameQueue', () => {
	it('keeps an early frame until the clock reaches it', () => {
		const q = new FrameQueue();
		const f = frame(10);
		q.push(f);
		expect(q.take(9.9)).toBeNull();
		expect(q.take(10)).toBe(f);
		expect(q.depth).toBe(0);
	});

	it('skips frames the clock left behind when a later one waits, keeping the last one', () => {
		const q = new FrameQueue({ lateMs: 80 });
		const a = frame(1);
		const b = frame(1.04);
		const c = frame(2);
		q.push(a);
		q.push(b);
		q.push(c);
		expect(q.take(1.11)).toBe(b);
		expect(a.closed).toBe(true);
		expect(q.take(3)).toBe(c);
		expect(c.closed).toBe(false);
	});

	it('drops the oldest beyond the cap and closes everything on clear', () => {
		const q = new FrameQueue({ max: 2 });
		const frames = [frame(1), frame(2), frame(3)];
		for (const f of frames) q.push(f);
		expect(frames[0]!.closed).toBe(true);
		expect(q.depth).toBe(2);
		q.clear();
		expect(frames.every((f) => f.closed)).toBe(true);
	});
});

describe('drawLoop', () => {
	it('runs only while the step says there is more, and wakes on a kick', () => {
		const pending: Array<() => void> = [];
		const raf = (cb: () => void) => pending.push(cb);
		let left = 2;
		const loop = drawLoop(() => (left -= 1) > 0, raf);
		loop.kick();
		loop.kick();
		expect(pending).toHaveLength(1);
		pending.shift()!();
		expect(pending).toHaveLength(1);
		pending.shift()!();
		expect(pending).toHaveLength(0);
		left = 1;
		loop.kick();
		expect(pending).toHaveLength(1);
		loop.stop();
		pending.shift()!();
		loop.kick();
		expect(pending).toHaveLength(0);
	});

	it('sleeps while paused with frames still queued, steps once per arriving frame, wakes on play', () => {
		const pending: Array<() => void> = [];
		const raf = (cb: () => void) => pending.push(cb);
		let steps = 0;
		const loop = drawLoop(() => {
			steps += 1;
			return true;
		}, raf);
		loop.kick();
		pending.shift()!();
		expect(pending).toHaveLength(1);
		loop.setPaused(true);
		pending.shift()!();
		expect(pending).toHaveLength(0);
		loop.kick();
		pending.shift()!();
		expect(pending).toHaveLength(0);
		expect(steps).toBe(3);
		loop.setPaused(false);
		expect(pending).toHaveLength(1);
		pending.shift()!();
		expect(pending).toHaveLength(1);
	});
});
