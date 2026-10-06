import { describe, expect, it } from 'vitest';
import { sameSite } from '#lib/paths.ts';
import { load } from './+page.ts';

const ORIGIN = 'https://iris.test';

const target = (to: string) => {
	try {
		void load({ url: new URL(`${ORIGIN}/login?${new URLSearchParams({ redirect: to })}`) } as Parameters<typeof load>[0]);
	} catch (e) {
		return e;
	}
};

describe('/login?redirect=', () => {
	it('goes on to a path on this site, its query and hash kept', () => {
		expect(target('/collection/c1?season=2#e3')).toMatchObject({ status: 308, location: '/collection/c1?season=2#e3' });
	});

	it('never leaves the site', () => {
		for (const to of ['//evil.example', '/\\evil.example', '/\\/evil.example', 'https://evil.example/', 'javascript:alert(1)', '']) {
			expect(target(to)).toMatchObject({ status: 308, location: '/' });
		}
		expect(sameSite(null, ORIGIN)).toBe('/');
	});
});
