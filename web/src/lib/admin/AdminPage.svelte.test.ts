import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { page } from 'vitest/browser';
import { session } from '#lib/session.svelte.ts';
import { ui } from '#lib/ui.svelte.ts';
import { stubApi } from '#lib/test/api.ts';
import '../../styles/app.css';
import { alex, leonard, noContent } from '../account/testing.ts';
import AdminGate from './AdminGate.svelte';
import AdminPage from './AdminPage.svelte';
import { html } from '#lib/test/snippet.ts';

const day = (d: number, h = 12) => new Date(2026, 9, d, h).toISOString();
const users = [
	{ id: 'u1', email: leonard.email, display_name: 'Léonard', is_admin: true, created_at: day(1) },
	{ id: 'u2', email: alex.email, display_name: 'Alex', is_admin: false, created_at: day(2) }
];
const later = new Date(Date.now() + 3 * 86_400_000).toISOString();
const invitations = [
	{ id: 'i1', created_at: day(3), created_by: 'u1', expires_at: later },
	{ id: 'i2', created_at: day(1), created_by: 'u1', expires_at: later, consumed_at: day(2), consumed_by: 'u2' }
];

function site(more: Record<string, unknown> = {}) {
	return stubApi({
		'GET /admin/users': users,
		'GET /admin/invitations': invitations,
		'GET /admin/active-sessions': [
			{
				user_id: 'u2',
				display_name: 'Alex',
				infohash: 'ab',
				file_idx: 0,
				file_path: 'Show/Severance.S02E04.mkv',
				position_seconds: 600,
				duration_seconds: 3000,
				started_at: new Date(Date.now() - 12 * 60_000).toISOString(),
				last_seen_at: new Date().toISOString(),
				state: 'paused',
				client: 'tv',
				client_version: '1.5.0',
				tmdb_verified: false
			}
		],
		'GET /admin/watch-history?limit=30': [],
		'GET /admin/storage': {
			used_bytes: 900,
			max_storage_bytes: 1000,
			threshold_bytes: 850,
			target_bytes: 700,
			threshold_pct: 85,
			target_pct: 70,
			torrent_count: 3,
			total_uploaded_bytes: 2000,
			total_downloaded_bytes: 1000
		},
		'GET /admin/remux': [],
		'GET /admin/audit-log?limit=50': [
			{ id: 1, action: 'user.password_reset', actor_id: 'u1', actor_display_name: 'Léonard', resource_type: 'user', created_at: day(5) }
		],
		...more
	});
}
const region = (name: string) => page.getByRole('region', { name });

describe('admin page', () => {
	beforeEach(() => session.signedIn(leonard));
	afterEach(() => {
		ui.toasts = [];
	});

	it('its sections, each named; live facts in words', async () => {
		site();
		await render(AdminPage);
		await expect.element(page.getByRole('heading', { level: 1, name: 'Admin' })).toBeVisible();
		for (const name of ['Users', 'Invitations', 'Now watching', 'Watch history', 'Storage', 'Maintenance', 'Audit log']) {
			await expect.element(region(name)).toBeVisible();
		}
		await expect.element(region('Now watching').getByText('Paused')).toBeVisible();
		await expect
			.element(region('Now watching').getByText(/20% watched, stopped at 10:00 · On the TV app 1.5.0 · for 12 min/))
			.toBeVisible();
		await expect.element(region('Storage').getByText('Past the clean-up line: the next clean-up frees space')).toBeVisible();
		await expect.element(region('Audit log').getByText(/Léonard set a new password/)).toBeVisible();
		await expect.element(region('Invitations').getByText(/Used .* by Alex/)).toBeVisible();
	});

	it('creates an invitation: its link has the focus, ready to copy', async () => {
		const api = site({ 'POST /admin/invitations': { id: 'i3', token: 'tok xyz', expires_at: later } });
		const write = vi.fn(async () => {});
		vi.spyOn(navigator.clipboard, 'writeText').mockImplementation(write);
		await render(AdminPage);
		await page.getByRole('button', { name: 'Create an invitation link' }).click();
		const link = page.getByLabelText('Invitation link');
		await expect.element(link).toHaveValue(`${location.origin}/register?token=tok%20xyz`);
		await expect.element(link).toHaveFocus();
		expect(api.sent('POST', '/admin/invitations')).toHaveLength(1);
		await page.getByRole('button', { name: 'Copy the link' }).click();
		expect(write).toHaveBeenCalledWith(`${location.origin}/register?token=tok%20xyz`);
	});

	it('revokes a waiting invitation after asking; a used one cannot be', async () => {
		const api = site({ 'DELETE /admin/invitations/i1': noContent() });
		await render(AdminPage);
		const revokes = region('Invitations').getByRole('button', { name: /^Revoke the invitation made/ });
		await expect.element(revokes).toHaveLength(1);
		await revokes.click();
		const dialog = page.getByRole('alertdialog', { name: 'Revoke this invitation?' });
		await expect.element(dialog.getByRole('button', { name: 'Keep' })).toBeVisible();
		await dialog.getByRole('button', { name: 'Revoke the invitation' }).click();
		await expect.poll(() => api.sent('DELETE', '/admin/invitations/i1')).toHaveLength(1);
		await expect.element(page.getByRole('heading', { name: 'Invitations' })).toHaveFocus();
	});

	it('deletes someone after asking, never oneself', async () => {
		const api = site({ 'DELETE /admin/users/u2': noContent() });
		await render(AdminPage);
		await expect.element(page.getByRole('button', { name: 'Delete the account of Léonard' })).not.toBeInTheDocument();
		await page.getByRole('button', { name: 'Delete the account of Alex' }).click();
		await page.getByRole('alertdialog', { name: 'Delete Alex’s account?' }).getByRole('button', { name: 'Delete the account' }).click();
		await expect.poll(() => api.sent('DELETE', '/admin/users/u2')).toHaveLength(1);
	});

	it('sets a new password for someone: too short is said under the field', async () => {
		const api = site({ 'POST /admin/users/u2/password': noContent() });
		await render(AdminPage);
		await page.getByRole('button', { name: 'Set a new password for Alex' }).click();
		const field = page.getByLabelText('New password', { exact: true });
		await field.fill('short');
		await page.getByRole('button', { name: 'Set the new password' }).click();
		await expect.element(field).toHaveAccessibleDescription('At least 8 characters. Use at least 8 characters.');
		await field.fill('long enough');
		await page.getByRole('button', { name: 'Set the new password' }).click();
		await expect.poll(() => api.sent('POST', '/admin/users/u2/password')[0]?.body).toEqual({ new_password: 'long enough' });
		await expect.element(page.getByRole('dialog')).not.toBeInTheDocument();
	});

	it('finds a person by name or email', async () => {
		site();
		await render(AdminPage);
		await page.getByLabelText('Find a person').fill('alex@');
		await expect.element(region('Users').getByRole('listitem')).toHaveLength(1);
		await expect.element(page.getByText('1 match of 2')).toBeVisible();
	});

	it('fits a 320 px screen: nothing wider than it', async () => {
		await page.viewport(320, 900);
		site();
		await render(AdminPage);
		await expect.element(page.getByRole('heading', { level: 1 })).toBeVisible();
		await expect.poll(() => document.querySelectorAll('.list-row').length).toBeGreaterThan(0);
		expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(320);
		await page.viewport(414, 896);
	});

	it('frees disk space after asking, and says what went', async () => {
		const api = site({
			'POST /admin/gc': {
				used_bytes_before: 900,
				used_bytes_after: 400,
				target_bytes: 700,
				threshold_bytes: 850,
				derived_freed_bytes: 0,
				evicted: [{ infohash: 'aa', name: 'Old.Film.mkv', freed_bytes: 500 }]
			}
		});
		await render(AdminPage);
		await page.getByRole('button', { name: 'Free space now' }).click();
		await page.getByRole('alertdialog', { name: 'Free space now?' }).getByRole('button', { name: 'Free space' }).click();
		await expect.poll(() => api.sent('POST', '/admin/gc')).toHaveLength(1);
		await expect.element(page.getByText('Old.Film.mkv')).toBeVisible();
	});
});

describe('admin gate', () => {
	it('a member is told the page is for admins', async () => {
		session.signedIn(alex);
		await render(AdminGate, { children: html('<p>Secret</p>') });
		await expect.element(page.getByText('Only an admin can open this page.')).toBeVisible();
		await expect.element(page.getByText('Secret')).not.toBeInTheDocument();
	});
});
