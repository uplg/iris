import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { page, userEvent } from 'vitest/browser';
import { session } from '#lib/session.svelte.ts';
import { ui } from '#lib/ui.svelte.ts';
import { stubApi } from '#lib/test/api.ts';
import { json } from '#lib/test/fetch.ts';
import '../../styles/app.css';
import { authenticator, leonard, noContent, passkey, registerRoutes } from './testing.ts';
import AccountPage from './AccountPage.svelte';

// the current route, as SvelteKit would say it
const route = vi.hoisted(() => ({ url: new URL('http://iris.test/account'), state: {}, params: {} }));
const nav = vi.hoisted(() => ({ goto: vi.fn(), replaceState: vi.fn() }));
vi.mock('$app/state', () => ({ page: route }));
vi.mock('$app/navigation', () => nav);

const iphone = passkey();
const mac = passkey({ id: 'k2', name: '', backed_up: false, last_used_at: new Date().toISOString() });
const tv = {
	jti: 'j1',
	label: 'Living room',
	kind: 'android-tv',
	issued_at: new Date(2026, 9, 1).toISOString(),
	expires_at: new Date(2027, 0, 1).toISOString()
};

/** The account's backend; `more` replaces or adds routes. */
function site(more: Record<string, unknown> = {}) {
	return stubApi({
		'GET /me/passkeys': [iphone, mac],
		'GET /me/devices': [],
		'GET /me/playback-preferences': { audio_language: 'fre', subtitle_language: 'off' },
		'GET /me/preferences': { languages: ['french'], genres: [], include_anime: false, onboarding_completed: true },
		'GET /genres': { genres: [{ id: 18, name: 'Drama' }] },
		'GET /languages': { languages: [{ value: 'french', label: 'French' }] },
		...more
	});
}
const region = (name: string) => page.getByRole('region', { name });
const keyRows = () => region('Passkeys').getByRole('listitem');

describe('account page', () => {
	beforeEach(() => {
		session.signedIn(leonard);
		route.url = new URL('http://iris.test/account');
	});
	afterEach(() => {
		ui.toasts = [];
	});

	it('one h1, sections named by their titles, the person in words', async () => {
		site();
		await render(AccountPage);
		await expect.element(page.getByRole('heading', { level: 1, name: 'Account' })).toBeVisible();
		for (const name of ['You', 'Passkeys', 'Password', 'Devices', 'Playback', 'Recommendations', 'Display']) {
			await expect.element(region(name)).toBeVisible();
		}
		await expect.element(region('You').getByText('leonard.k@example.com')).toBeVisible();
		await expect.element(region('You').getByText('Admin')).toBeVisible();
	});

	it('lists my passkeys: name (or a word for none), synced in words, added and last used', async () => {
		site();
		await render(AccountPage);
		await expect.element(keyRows()).toHaveLength(2);
		await expect.element(keyRows().nth(0).getByText('iPhone')).toBeVisible();
		await expect.element(keyRows().nth(0).getByText('Synced')).toBeVisible();
		await expect
			.element(
				keyRows()
					.nth(0)
					.getByText(/Added .+ · Never used yet/)
			)
			.toBeVisible();
		await expect.element(keyRows().nth(1).getByText('Unnamed passkey')).toBeVisible();
		await expect
			.element(
				keyRows()
					.nth(1)
					.getByText(/Last used today at/)
			)
			.toBeVisible();
		await expect.element(keyRows().nth(1).getByText('Synced')).not.toBeInTheDocument();
		await expect.element(region('Passkeys').getByText(/Your password keeps working/)).toBeVisible();
	});

	it('renames a passkey in place; the focus goes back to its Rename button', async () => {
		const api = site({ 'PATCH /me/passkeys/k1': noContent() });
		await render(AccountPage);
		await page.getByRole('button', { name: 'Rename iPhone' }).click();
		const field = page.getByLabelText('Name of the passkey');
		await expect.element(field).toHaveFocus();
		await field.fill('My iPhone');
		await region('Passkeys').getByRole('button', { name: 'Save' }).click();
		await expect.element(field).not.toBeInTheDocument();
		expect(api.sent('PATCH', '/me/passkeys/k1')[0].body).toEqual({ name: 'My iPhone' });
		await expect.element(page.getByRole('button', { name: 'Rename iPhone' })).toHaveFocus();
	});

	it('removes a passkey after asking, the last one too (the password stays)', async () => {
		const api = site({ 'GET /me/passkeys': [iphone], 'DELETE /me/passkeys/k1': noContent() });
		await render(AccountPage);
		await page.getByRole('button', { name: 'Remove iPhone' }).click();
		const dialog = page.getByRole('alertdialog', { name: 'Remove “iPhone”?' });
		await expect.element(dialog.getByText(/You keep signing in with your password/)).toBeVisible();
		await expect.element(dialog.getByRole('button', { name: 'Keep' })).toBeVisible();
		api.routes['GET /me/passkeys'] = [];
		await dialog.getByRole('button', { name: 'Remove the passkey' }).click();
		await expect.poll(() => ui.toasts.map((t) => t.text)).toContain('Passkey “iPhone” removed.');
		expect(api.sent('DELETE', '/me/passkeys/k1')).toHaveLength(1);
		await expect.element(region('Passkeys').getByText('No passkey yet.')).toBeVisible();
		await expect.element(page.getByRole('heading', { name: 'Passkeys' })).toHaveFocus();
	});

	it('adds a passkey from this device', async () => {
		authenticator();
		const api = site(registerRoutes());
		await render(AccountPage);
		await page.getByRole('button', { name: 'Add a passkey' }).click();
		await expect.poll(() => ui.toasts.map((t) => t.text)).toContain('Passkey added as “MacBook”.');
		expect(api.sent('POST', '/me/passkeys/register/start')).toHaveLength(1);
		expect(api.sent('POST', '/me/passkeys/register/finish')[0].body).toMatchObject({ ceremony: 'c2' });
	});

	it('a wrong current password is said under it, with the focus on it', async () => {
		const api = site({ 'POST /me/password': json({ error: 'unauthorized', message: 'unauthorized' }, 401), 'POST /auth/refresh': leonard });
		await render(AccountPage);
		await page.getByLabelText('Current password', { exact: true }).fill('nope');
		await page.getByLabelText('New password', { exact: true }).fill('long enough');
		await page.getByRole('button', { name: 'Change my password' }).click();
		const current = page.getByLabelText('Current password', { exact: true });
		await expect.element(current).toHaveAccessibleDescription('This is not your current password.');
		await expect.element(current).toHaveFocus();
		expect(api.sent('POST', '/me/password')[0].body).toEqual({ old_password: 'nope', new_password: 'long enough' });
		expect(ui.toasts).toEqual([]);
	});

	it('a short new password is said under it, nothing sent; the fields can be shown', async () => {
		const api = site();
		await render(AccountPage);
		await page.getByLabelText('Current password', { exact: true }).fill('old secret');
		await page.getByLabelText('New password', { exact: true }).fill('short');
		await page.getByRole('button', { name: 'Change my password' }).click();
		await expect
			.element(page.getByLabelText('New password', { exact: true }))
			.toHaveAccessibleDescription('At least 8 characters. Use at least 8 characters.');
		expect(api.sent('POST', '/me/password')).toEqual([]);
		await page.getByRole('button', { name: 'Show new password' }).click();
		await expect.element(page.getByLabelText('New password', { exact: true })).toHaveAttribute('type', 'text');
		await expect.element(page.getByLabelText('New password', { exact: true })).toHaveAttribute('autocomplete', 'new-password');
	});

	it('changes the password, and says every device signs in again', async () => {
		const api = site({ 'POST /me/password': noContent() });
		await render(AccountPage);
		await page.getByLabelText('Current password', { exact: true }).fill('old secret');
		await page.getByLabelText('New password', { exact: true }).fill('new secret!');
		await page.getByRole('button', { name: 'Change my password' }).click();
		await expect
			.poll(() => ui.toasts.map((t) => t.text))
			.toContain('Password changed. Your devices, this one included, will ask you to sign in again with it.');
		expect(api.sent('POST', '/me/password')).toHaveLength(1);
		await expect.element(page.getByLabelText('Current password', { exact: true })).toHaveValue('');
	});

	it('pairs a TV by its code: the list is read again until the TV is in, no timer of ours', async () => {
		let reads = 0;
		const api = site({
			// the TV signs in on the second read after the code is accepted
			'GET /me/devices': () => (reads++ >= 2 ? [tv] : []),
			'POST /me/devices': noContent()
		});
		await render(AccountPage);
		await expect.element(region('Devices').getByText('No paired devices yet.')).toBeVisible();
		await page.getByLabelText('Pairing code').fill('wx7k-abcd');
		await expect.element(page.getByLabelText('Pairing code')).toHaveValue('WX7K-ABCD');
		await page.getByLabelText('Device name (optional)').fill('Living room');
		await page.getByRole('button', { name: 'Pair the TV' }).click();
		expect(api.sent('POST', '/me/devices')[0].body).toEqual({ code: 'WX7K-ABCD', label: 'Living room' });
		// accepted, the TV not in yet: the wait is said, with a way out
		await expect.element(page.getByRole('button', { name: 'Stop waiting' })).toBeVisible();
		await expect.poll(() => ui.toasts.map((t) => t.text), { timeout: 6000 }).toContain('Your TV is paired and signed in.');
		await expect.element(region('Devices').getByText('Living room')).toBeVisible();
		await expect.element(region('Devices').getByText('Android TV')).toBeVisible();
		// the wait ends with the TV's arrival, not with a clock
		await expect.element(page.getByRole('button', { name: 'Stop waiting' })).not.toBeInTheDocument();
	});

	it('a re-paired TV is seen arriving although the count of devices stays the same', async () => {
		let reads = 0;
		// the TV's old row is replaced by its new one
		const again = { ...tv, jti: 'j2' };
		site({ 'GET /me/devices': () => (reads++ >= 2 ? [again] : [tv]), 'POST /me/devices': noContent() });
		await render(AccountPage);
		await page.getByLabelText('Pairing code').fill('wx7k-abcd');
		await page.getByRole('button', { name: 'Pair the TV' }).click();
		await expect.poll(() => ui.toasts.map((t) => t.text), { timeout: 6000 }).toContain('Your TV is paired and signed in.');
	});

	it('a refused code is said under the code field', async () => {
		site({ 'POST /me/devices': json({ error: 'bad_request', message: 'bad request: invalid or expired code' }, 400) });
		await render(AccountPage);
		await page.getByLabelText('Pairing code').fill('ABCD-EFGH');
		await page.getByRole('button', { name: 'Pair the TV' }).click();
		const field = page.getByLabelText('Pairing code');
		await expect.element(field).toHaveAccessibleDescription('bad request: invalid or expired code');
		await expect.element(field).toHaveFocus();
	});

	it('a code from the TV’s link fills the field, then leaves the address', async () => {
		route.url = new URL('http://iris.test/account?pair=abcd-efgh');
		site();
		await render(AccountPage);
		await expect.element(page.getByLabelText('Pairing code')).toHaveValue('ABCD-EFGH');
		await expect.poll(() => String(nav.goto.mock.calls.at(-1)?.[0])).toBe('http://iris.test/account');
		expect(nav.goto.mock.calls.at(-1)?.[1]).toMatchObject({ shallow: true, replace: true });
	});

	it('signs a device out after asking', async () => {
		const api = site({ 'GET /me/devices': [tv], 'DELETE /me/devices/j1': noContent() });
		await render(AccountPage);
		await page.getByRole('button', { name: 'Sign out Living room' }).click();
		await page.getByRole('alertdialog').getByRole('button', { name: 'Sign the device out' }).click();
		await expect.poll(() => api.sent('DELETE', '/me/devices/j1')).toHaveLength(1);
	});

	it('the playback languages: saved values in words, a change sent whole', async () => {
		const api = site({ 'PUT /me/playback-preferences': noContent() });
		await render(AccountPage);
		await expect.element(region('Playback').getByText('French')).toBeVisible();
		await expect.element(region('Playback').getByText('No subtitles')).toBeVisible();
		await region('Playback')
			.getByRole('button', { name: /^Audio/ })
			.click();
		await page.getByRole('option', { name: 'English' }).click();
		await expect
			.poll(() => api.sent('PUT', '/me/playback-preferences')[0]?.body)
			.toEqual({ audio_language: 'en', subtitle_language: 'off' });
	});

	it('recommendations: nothing to save until changed; saved, what the server keeps shows', async () => {
		const api = site({
			'PUT /me/preferences': { languages: ['french'], genres: [18], include_anime: true, onboarding_completed: true }
		});
		await render(AccountPage);
		const save = page.getByRole('button', { name: 'Save recommendations' });
		await expect.element(save).toHaveAccessibleDescription('Nothing changed since the last save.');
		await page.getByRole('button', { name: 'Drama' }).click();
		await page.getByRole('button', { name: 'Anime' }).click();
		await expect.element(page.getByRole('button', { name: 'Drama' })).toHaveAttribute('aria-pressed', 'true');
		await save.click();
		await expect
			.poll(() => api.sent('PUT', '/me/preferences')[0]?.body)
			.toEqual({
				languages: ['french'],
				genres: [18],
				include_anime: true,
				onboarding_completed: true
			});
		await expect.element(save).toHaveAccessibleDescription('Nothing changed since the last save.');
	});

	it('fits a 320 px screen: nothing wider than it', async () => {
		await page.viewport(320, 900);
		site({ 'GET /me/devices': [tv] });
		await render(AccountPage);
		await expect.element(page.getByRole('heading', { level: 1 })).toBeVisible();
		await expect.poll(() => document.querySelectorAll('.list-row').length).toBeGreaterThan(0);
		expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(320);
		await page.viewport(414, 896);
	});

	it('the display theme, the same choice as the header’s', async () => {
		site();
		await render(AccountPage);
		await region('Display').getByRole('radio', { name: 'Dark' }).click();
		expect(ui.theme).toBe('dark');
		ui.setTheme('system');
	});

	it('renames me: the session takes the name the server gives back', async () => {
		const api = site({ 'POST /me/display-name': noContent(), 'GET /me': { ...leonard, display_name: 'Leo' } });
		await render(AccountPage);
		const field = page.getByLabelText('Display name');
		await field.fill('Leo');
		await userEvent.keyboard('{Enter}');
		await expect.poll(() => session.user?.display_name).toBe('Leo');
		expect(api.sent('POST', '/me/display-name')[0].body).toEqual({ display_name: 'Leo' });
	});
});
