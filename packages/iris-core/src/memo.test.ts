import { describe, expect, it } from 'vitest';
import { onceUntilFailure } from './memo';

describe('onceUntilFailure', () => {
	it('loads once and shares the answer', async () => {
		let calls = 0;
		const get = onceUntilFailure(async () => ++calls);
		expect(await Promise.all([get(), get()])).toEqual([1, 1]);
		expect(await get()).toBe(1);
	});

	it('asks again after a failure', async () => {
		let calls = 0;
		const get = onceUntilFailure(async () => {
			calls += 1;
			if (calls === 1) throw new Error('blip');
			return calls;
		});
		await expect(get()).rejects.toThrow('blip');
		expect(await get()).toBe(2);
		expect(await get()).toBe(2);
	});
});
