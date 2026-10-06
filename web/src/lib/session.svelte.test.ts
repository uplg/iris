import { describe, expect, it } from 'vitest';
import { queryClient } from '#lib/query.ts';
import { KEYS } from '#lib/queries.ts';
import { backToResults, rememberSearch } from '#lib/search/cache.ts';
import { stubApi } from '#lib/test/api.ts';
import { ui } from '#lib/ui.svelte.ts';
import { Session } from '@iris/api/session';
import { accountChanged, session } from './session.svelte.ts';

const user = (id: string) => ({ id, email: `${id}@example.com`, display_name: id, is_admin: false });
const signedIn = (id: string) => ({ status: 'signed_in', user: user(id) }) as const;

describe('the session bootstrap', () => {
	it('a signed-out visitor costs one refresh, and is said signed out once', async () => {
		const api = stubApi({
			'GET /me': () => new Response(null, { status: 401 }),
			'POST /auth/refresh': () => new Response(null, { status: 401 })
		});
		const core = new Session();
		const said: string[] = [];
		core.subscribe((s) => said.push(s.status));
		core.start();
		await expect.poll(() => core.state.status).toBe('signed_out');
		await new Promise((resolve) => requestAnimationFrame(resolve));
		core.stop();
		expect(api.sent('POST', '/auth/refresh')).toHaveLength(1);
		expect(said).toEqual(['loading', 'signed_out']);
	});
});

describe('the session forgets an account’s answers', () => {
	it('when it leaves or another one comes in, never on the first answer', () => {
		expect(accountChanged({ status: 'loading', retrying: false }, signedIn('a'))).toBe(false);
		expect(accountChanged(signedIn('a'), signedIn('a'))).toBe(false);
		expect(accountChanged(signedIn('a'), signedIn('b'))).toBe(true);
		expect(accountChanged(signedIn('a'), { status: 'signed_out' })).toBe(true);
		expect(accountChanged({ status: 'signed_out' }, signedIn('b'))).toBe(false);
	});

	it('another person signing in reads nothing of the last one’s', () => {
		session.signedIn(user('a'));
		queryClient.setQueryData(KEYS.preferences, { languages: ['french'], genres: [28], onboarding_completed: true });
		rememberSearch('/search?q=secret');
		session.signedIn(user('a'));
		expect(queryClient.getQueryData(KEYS.preferences)).toBeDefined();
		session.signedIn(user('b'));
		expect(queryClient.getQueryData(KEYS.preferences)).toBeUndefined();
		expect(backToResults()).toBe('/search');
	});

	it('signing out clears the cache; a failed sign-out is said and keeps the person in', async () => {
		session.signedIn(user('a'));
		queryClient.setQueryData(KEYS.watchlist, []);
		const api = stubApi({ 'POST /auth/logout': () => new Response(null, { status: 503 }) });
		await session.logout();
		expect(session.state.status).toBe('signed_in');
		expect(queryClient.getQueryData(KEYS.watchlist)).toEqual([]);
		expect(ui.toasts.at(-1)?.warn).toBe(true);
		api.routes['POST /auth/logout'] = () => new Response(null, { status: 204 });
		await session.logout();
		expect(session.state.status).toBe('signed_out');
		expect(queryClient.getQueryData(KEYS.watchlist)).toBeUndefined();
	});
});
