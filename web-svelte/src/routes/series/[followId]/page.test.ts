import { describe, expect, it } from 'vitest';
import { load } from './+page.ts';

describe('/series/[followId]', () => {
	it('moves to the collection with the same id', () => {
		let thrown: unknown;
		try {
			void load({ params: { followId: 'c1' } } as Parameters<typeof load>[0]);
		} catch (e) {
			thrown = e;
		}
		expect(thrown).toMatchObject({ status: 308, location: '/collection/c1' });
	});
});
