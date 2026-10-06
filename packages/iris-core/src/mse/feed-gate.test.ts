import { describe, expect, it } from 'vitest';
import { FeedGate } from './feed-gate';

const settled = async (p: Promise<void>) => {
	let done = false;
	void p.then(() => (done = true));
	await Promise.resolve();
	await Promise.resolve();
	return done;
};

describe('FeedGate', () => {
	it('lets a ready loop through at once', async () => {
		const gate = new FeedGate();
		expect(await settled(gate.wait(() => true))).toBe(true);
		expect(gate.size).toBe(0);
	});

	it('parks a loop until a notify finds it ready, and says why meanwhile', async () => {
		const gate = new FeedGate();
		let room = false;
		const p = gate.wait(
			() => room,
			() => 'room'
		);
		expect(gate.parked).toBe('room');
		gate.notify();
		expect(await settled(p)).toBe(false);
		room = true;
		gate.notify();
		expect(await settled(p)).toBe(true);
		expect(gate.parked).toBeNull();
		expect(gate.size).toBe(0);
	});

	it('flush wakes every parked loop, ready or not', async () => {
		const gate = new FeedGate();
		const a = gate.wait(() => false);
		const b = gate.wait(() => false);
		gate.flush();
		expect(await settled(a)).toBe(true);
		expect(await settled(b)).toBe(true);
		expect(gate.size).toBe(0);
	});
});
