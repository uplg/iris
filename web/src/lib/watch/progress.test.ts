import { describe, expect, it, vi } from 'vitest';
import { ProgressSaver } from './progress.ts';

describe('progress heartbeats', () => {
	it('buffering is said once per change, never before the file has started', () => {
		const put = vi.fn(async () => {});
		const saver = new ProgressSaver('ab', 0, { put, beacon: vi.fn() });
		saver.busy(true);
		expect(put).not.toHaveBeenCalled();
		saver.busy(false);
		saver.timeUpdate(30);
		put.mockClear();

		saver.busy(true);
		saver.busy(true);
		expect(put).toHaveBeenCalledTimes(1);
		expect(put.mock.calls[0]).toEqual(['ab', 0, expect.objectContaining({ position_seconds: 30, playing: true, buffering: true })]);
		saver.busy(false);
		expect(put).toHaveBeenCalledTimes(2);
		expect(put.mock.calls[1]).toEqual(['ab', 0, expect.objectContaining({ buffering: false })]);
	});
});
