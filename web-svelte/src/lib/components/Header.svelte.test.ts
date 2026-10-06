import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { page as screen } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { session } from '#lib/session.svelte.ts';
import { ui } from '#lib/ui.svelte.ts';
import Header from './Header.svelte';

// the current route, as SvelteKit would say it
const route = vi.hoisted(() => ({ url: new URL('http://iris.test/') }));
vi.mock('$app/state', () => ({ page: route }));

const at = (path: string) => (route.url = new URL(path, 'http://iris.test'));
const user = (is_admin: boolean) => ({ id: 'u1', email: 'leonard@example.com', display_name: 'Leonard', is_admin });

/** The links of the header's navigation (the bottom bar repeats them under 600 px). */
const nav = () => screen.getByRole('navigation', { name: 'Main' }).first();

describe('Header', () => {
	beforeEach(() => {
		session.signedIn(user(false));
		at('/');
	});
	afterEach(() => ui.setTheme('system'));

	it('marks the current destination; a collection page belongs to the library', async () => {
		at('/collection/42');
		await render(Header);
		await expect.element(nav().getByRole('link', { name: 'Library' })).toHaveAttribute('aria-current', 'page');
		await expect.element(nav().getByRole('link', { name: 'Home' })).not.toHaveAttribute('aria-current');
		await expect.element(nav().getByRole('link', { name: 'Search' })).toHaveAttribute('href', '/search');
	});

	it('opens the account panel from the name; admin only for admins', async () => {
		await render(Header);
		await screen.getByRole('button', { name: 'Leonard: account, theme, sign out' }).click();
		const panel = screen.getByRole('dialog', { name: 'Leonard' });
		await expect.element(panel.getByRole('group', { name: 'Theme' })).toBeVisible();
		await expect.element(panel.getByRole('link', { name: 'History' })).toBeVisible();
		await expect.element(panel.getByRole('link', { name: 'Admin' })).not.toBeInTheDocument();
	});

	it('shows admin to an admin', async () => {
		session.signedIn(user(true));
		await render(Header);
		await screen.getByRole('button', { name: 'Leonard: account, theme, sign out' }).click();
		await expect.element(screen.getByRole('link', { name: 'Admin' })).toBeVisible();
	});

	it('switches the theme; the current one is checked', async () => {
		await render(Header);
		await screen.getByRole('button', { name: 'Leonard: account, theme, sign out' }).click();
		await expect.element(screen.getByRole('radio', { name: 'System' })).toHaveAttribute('aria-checked', 'true');
		const dark = screen.getByRole('radio', { name: 'Dark' });
		await dark.click();
		expect(ui.theme).toBe('dark');
		await expect.element(dark).toHaveAttribute('aria-checked', 'true');
	});
});
