import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { page } from 'vitest/browser';
import { session } from '#lib/session.svelte.ts';
import { stubApi } from '#lib/test/api.ts';
import { json } from '#lib/test/fetch.ts';
import '../../styles/app.css';
import { alex } from './testing.ts';
import RegisterPage from './RegisterPage.svelte';

const route = vi.hoisted(() => ({ url: new URL('http://iris.test/register?token=tok-123'), state: {}, params: {} }));
const nav = vi.hoisted(() => ({ goto: vi.fn(), replaceState: vi.fn() }));
vi.mock('$app/state', () => ({ page: route }));
vi.mock('$app/navigation', () => nav);

const password = () => page.getByLabelText('Password', { exact: true });

describe('register', () => {
	beforeEach(() => {
		route.url = new URL('http://iris.test/register?token=tok-123');
		nav.goto.mockClear();
	});

	it('the door: one h1, the code from the invite link already in, one big action', async () => {
		stubApi({});
		await render(RegisterPage);
		await expect.element(page.getByRole('heading', { level: 1, name: 'Create your account' })).toBeVisible();
		await expect.element(page.getByLabelText('Invitation code')).toHaveValue('tok-123');
		await expect.element(password()).toHaveAttribute('autocomplete', 'new-password');
		await expect.element(page.getByRole('button', { name: 'Create my account' })).toHaveClass(/big/);
	});

	it('creates the account and signs in, then Home', async () => {
		const api = stubApi({ 'POST /auth/register': alex });
		await render(RegisterPage);
		await page.getByLabelText('Email').fill('alex@example.com');
		await password().fill('long enough');
		await page.getByRole('button', { name: 'Create my account' }).click();
		await expect.poll(() => session.user?.id).toBe('u2');
		expect(api.sent('POST', '/auth/register')[0].body).toEqual({
			invite_token: 'tok-123',
			email: 'alex@example.com',
			password: 'long enough'
		});
		await expect.poll(() => nav.goto.mock.calls.at(-1)?.[0]).toBe('/');
	});

	it('a short password is said under it, nothing sent; it can be shown', async () => {
		const api = stubApi({});
		await render(RegisterPage);
		await page.getByLabelText('Email').fill('alex@example.com');
		await password().fill('short');
		await page.getByRole('button', { name: 'Create my account' }).click();
		await expect.element(password()).toHaveAccessibleDescription(/Use at least 8 characters\./);
		await expect.element(password()).toHaveFocus();
		expect(api.sent('POST', '/auth/register')).toEqual([]);
		await page.getByRole('button', { name: 'Show' }).click();
		await expect.element(password()).toHaveAttribute('type', 'text');
	});

	it('a used invitation is said under the code, with what to do', async () => {
		stubApi({ 'POST /auth/register': json({ error: 'bad_request', message: 'bad request: invalid or expired invitation' }, 400) });
		await render(RegisterPage);
		await page.getByLabelText('Email').fill('alex@example.com');
		await password().fill('long enough');
		await page.getByRole('button', { name: 'Create my account' }).click();
		const code = page.getByLabelText('Invitation code');
		await expect.element(code).toHaveAccessibleDescription('This invitation code is not valid any more. Ask for a new invitation link.');
		await expect.element(code).toHaveFocus();
	});

	it('an email already in use is said under the email', async () => {
		stubApi({ 'POST /auth/register': json({ error: 'conflict', message: 'conflict: email already registered' }, 409) });
		await render(RegisterPage);
		await page.getByLabelText('Email').fill('alex@example.com');
		await password().fill('long enough');
		await page.getByRole('button', { name: 'Create my account' }).click();
		await expect.element(page.getByLabelText('Email')).toHaveAccessibleDescription('An account already uses this email. Sign in instead.');
	});
});
