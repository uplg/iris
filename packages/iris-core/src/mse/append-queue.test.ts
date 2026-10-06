import { describe, expect, it, vi } from 'vitest';
import { AppendQueue } from './append-queue';

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
});
