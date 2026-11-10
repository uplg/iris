import { describe, expect, it, vi } from 'vitest';
import { AppendQueue, WEDGED_OP_MS } from './append-queue';

class FakeSourceBuffer extends EventTarget {
	updating = false;
	appended: Uint8Array[] = [];
	quota = false;
	appendBuffer(data: Uint8Array) {
		if (this.quota) throw new DOMException('full', 'QuotaExceededError');
		this.appended.push(data);
		this.updating = true;
	}
	land() {
		this.updating = false;
		this.dispatchEvent(new Event('updateend'));
	}
	aborted = 0;
	abort() {
		this.aborted += 1;
		this.land();
	}
	/** The engine's own eviction, started outside the queue. */
	remove() {
		this.updating = true;
		this.dispatchEvent(new Event('updatestart'));
	}
}

const chunk = (n: number) => new Uint8Array([n]);

function setup(proxied = false) {
	const sb = new FakeSourceBuffer();
	const onQuota = vi.fn();
	const onError = vi.fn();
	const onSettled = vi.fn();
	let alive = true;
	const queue = new AppendQueue({ alive: () => alive, onQuota, onError, onSettled, proxied });
	queue.attach(sb as unknown as SourceBuffer);
	return { sb, queue, onQuota, onError, onSettled, kill: () => (alive = false) };
}

describe('AppendQueue', () => {
	it('hands one chunk at a time to an idle buffer, the view itself', () => {
		const { sb, queue } = setup();
		const a = chunk(1);
		queue.push(a, chunk(2));
		expect(queue.pump()).toBe(true);
		expect(queue.pump()).toBe(false);
		expect(sb.appended).toEqual([a]);
		expect(sb.appended[0]).toBe(a);
		sb.land();
		expect(queue.pump()).toBe(true);
		expect(queue.length).toBe(0);
	});

	it('keeps the chunk on a quota error and lets the engine free room, without retrying', () => {
		const { sb, queue, onQuota } = setup();
		sb.quota = true;
		queue.push(chunk(1));
		expect(queue.pump()).toBe(false);
		expect(onQuota).toHaveBeenCalledTimes(1);
		expect(queue.length).toBe(1);
		sb.quota = false;
		expect(queue.pump()).toBe(true);
	});

	it('reports any other append failure', () => {
		const { sb, queue, onError } = setup();
		sb.appendBuffer = () => {
			throw new DOMException('closed', 'InvalidStateError');
		};
		queue.push(chunk(1));
		queue.pump();
		expect(onError).toHaveBeenCalledTimes(1);
	});

	it('appends nothing once the engine is gone', () => {
		const { sb, queue, kill } = setup();
		kill();
		queue.push(chunk(1));
		expect(queue.pump()).toBe(false);
		expect(sb.appended).toHaveLength(0);
	});

	it('proxied: copies each chunk, serialises on its own flag and drains on every settle', () => {
		const { sb, queue, onSettled } = setup(true);
		sb.appendBuffer = function (this: FakeSourceBuffer, data: Uint8Array) {
			this.appended.push(data);
		};
		const a = chunk(1);
		queue.push(a, chunk(2), chunk(3));
		queue.pump();
		expect(queue.pump()).toBe(false);
		expect(sb.appended[0]).not.toBe(a);
		expect(sb.appended[0]).toEqual(a);
		sb.dispatchEvent(new Event('updateend'));
		sb.dispatchEvent(new Event('abort'));
		expect(sb.appended).toHaveLength(3);
		expect(onSettled).toHaveBeenCalledTimes(2);
	});

	describe('a wedged op', () => {
		const at = (ms: number) => vi.spyOn(performance, 'now').mockReturnValue(ms);

		it('leaves an append that is merely in flight alone', () => {
			const { sb, queue } = setup();
			at(1000);
			queue.push(chunk(1), chunk(2));
			queue.pump();
			at(1000 + WEDGED_OP_MS - 1);
			expect(queue.unwedge()).toBe(false);
			expect(sb.aborted).toBe(0);
			vi.restoreAllMocks();
		});

		it('aborts an append still in flight after the limit, and the next one lands', () => {
			const { sb, queue } = setup();
			at(1000);
			queue.push(chunk(1), chunk(2));
			queue.pump();
			at(1000 + WEDGED_OP_MS);
			expect(queue.opAgeMs).toBe(WEDGED_OP_MS);
			expect(queue.unwedge()).toBe(true);
			expect(sb.aborted).toBe(1);
			expect(queue.opAgeMs).toBe(0);
			expect(queue.pump()).toBe(true);
			vi.restoreAllMocks();
		});

		it("ages each op from its own start, the engine's remove included", () => {
			const { sb, queue } = setup();
			at(1000);
			queue.push(chunk(1));
			queue.pump();
			sb.land();
			at(9000);
			sb.remove();
			at(9010);
			expect(queue.opAgeMs).toBe(10);
			expect(queue.unwedge()).toBe(false);
			vi.restoreAllMocks();
		});

		it('has nothing to abort on an idle buffer', () => {
			const { sb, queue } = setup();
			expect(queue.opAgeMs).toBe(0);
			expect(queue.unwedge()).toBe(false);
			expect(sb.aborted).toBe(0);
		});
	});
});
