import { describe, expect, it } from 'vitest';
import { deviceLossAction } from './webgpu-renderer';

describe('deviceLossAction', () => {
	it('ignores our own dispose', () => {
		expect(deviceLossAction('destroyed', { disposed: false, recreated: false })).toBe('ignore');
		expect(deviceLossAction('unknown', { disposed: true, recreated: false })).toBe('ignore');
	});

	it('re-requests a device once for any other loss (a hidden tab reaped, a driver reset)', () => {
		expect(deviceLossAction('unknown', { disposed: false, recreated: false })).toBe('recreate');
		expect(deviceLossAction(undefined, { disposed: false, recreated: false })).toBe('recreate');
	});

	it('reports a second loss so the page demotes the tier', () => {
		expect(deviceLossAction('unknown', { disposed: false, recreated: true })).toBe('fail');
	});
});
