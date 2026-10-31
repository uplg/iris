import { createRawSnippet } from 'svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { page } from 'vitest/browser';
import { queryClient } from '#lib/query.ts';
import { session } from '#lib/session.svelte.ts';
import { stubApi } from '#lib/test/api.ts';
import { chosen } from './chosen.svelte.ts';
import ProfileGate from './ProfileGate.svelte';

const user = { id: 'u1', email: 'a@example.com', display_name: 'Vincent', is_admin: false };
const children = createRawSnippet(() => ({ render: () => '<p>The app</p>' }));
const vincent = { id: 'u1', name: 'Vincent', main: true, current: true };
const muriel = { id: 'u2', name: 'Muriel', main: false, current: false };

describe('who is watching', () => {
	beforeEach(() => {
		queryClient.clear();
		chosen.set(false);
		session.signedIn(user);
	});

	it('one profile goes straight in', async () => {
		stubApi({ 'GET /me/profiles': [vincent] });
		render(ProfileGate, { children });
		await expect.element(page.getByText('The app')).toBeVisible();
	});

	it('a failed read never locks anyone out', async () => {
		stubApi({ 'GET /me/profiles': () => new Response(null, { status: 500 }) });
		render(ProfileGate, { children });
		await expect.element(page.getByText('The app')).toBeVisible();
	});

	it('several ask once, and going onto another one changes the session', async () => {
		const api = stubApi({
			'GET /me/profiles': [vincent, muriel],
			'POST /auth/switch-profile': { ...user, id: 'u2', display_name: 'Muriel', account_id: 'u1' }
		});
		render(ProfileGate, { children });
		await expect.element(page.getByRole('heading', { name: 'Who’s watching?' })).toBeVisible();
		await expect.element(page.getByText('The app')).not.toBeInTheDocument();
		await page.getByRole('button', { name: 'Muriel' }).click();
		await expect.element(page.getByText('The app')).toBeVisible();
		expect(api.sent('POST', '/auth/switch-profile')[0]?.body).toEqual({ profile_id: 'u2' });
		expect(session.user?.display_name).toBe('Muriel');
	});

	it('staying on the current one sends nothing', async () => {
		const api = stubApi({ 'GET /me/profiles': [vincent, muriel] });
		render(ProfileGate, { children });
		await page.getByRole('button', { name: 'Vincent' }).click();
		await expect.element(page.getByText('The app')).toBeVisible();
		expect(api.sent('POST', '/auth/switch-profile')).toHaveLength(0);
	});
});
