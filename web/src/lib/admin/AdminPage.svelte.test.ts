import axe from 'axe-core';
import { IRIS_WEB_VERSION } from '@iris/api/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { page, userEvent } from 'vitest/browser';
import { session } from '#lib/session.svelte.ts';
import { ui } from '#lib/ui.svelte.ts';
import { STORAGE } from '#lib/storage.ts';
import { deferred } from '#lib/test/api.ts';
import '../../styles/app.css';
import { alex, leonard, noContent } from '../account/testing.ts';
import AdminGate from './AdminGate.svelte';
import AdminHarness from './AdminHarness.svelte';
import { html } from '#lib/test/snippet.ts';
import { household, people, plays, sessions, trackers } from './testing.ts';

const route = vi.hoisted(() => ({ url: new URL('http://iris.test/admin') }));
const nav = vi.hoisted(() => ({ goto: vi.fn(), replaceState: vi.fn() }));
vi.mock('$app/state', () => ({ page: route }));
vi.mock('$app/navigation', () => nav);

const at = (path: string) => (route.url = new URL(path, 'http://iris.test'));
const region = (name: string) => page.getByRole('region', { name, exact: true });
const historyCalls = (api: ReturnType<typeof household>) => api.calls.filter((c) => c.path.startsWith('/admin/watch-history'));
/** The rows of the household's history on the page. */
const playRows = () => document.querySelectorAll('#plays-title ~ * li.play, section[aria-labelledby="plays-title"] li.play');

describe('admin page', () => {
	beforeEach(() => {
		session.signedIn({ ...leonard, id: people[0].id });
		localStorage.clear();
		at('/admin');
		nav.replaceState.mockClear();
	});
	afterEach(() => {
		ui.toasts = [];
	});

	it('opens on the activity; a tab changes the view, kept in the URL and remembered', async () => {
		household();
		await render(AdminHarness);
		await expect.element(page.getByRole('heading', { level: 1, name: 'Admin' })).toBeVisible();
		await expect.element(page.getByRole('tab', { name: 'Activity' })).toHaveAttribute('aria-selected', 'true');
		await expect.element(region('Now watching')).toBeVisible();
		await expect.element(region('Watch history')).toBeVisible();
		// the views not shown read nothing
		expect(document.querySelector('#trackers-title')).toBeNull();

		await page.getByRole('tab', { name: 'System' }).click();
		await expect.element(region('Trackers')).toBeVisible();
		await expect.element(region('Disk')).toBeVisible();
		expect(String(nav.replaceState.mock.lastCall?.[0])).toBe('http://iris.test/admin?view=system');
		expect(localStorage.getItem(STORAGE.adminView)).toBe('system');
	});

	it('the URL picks the view; without one, the remembered view opens', async () => {
		household();
		at('/admin?view=log');
		const first = await render(AdminHarness);
		await expect.element(region('Audit log')).toBeVisible();
		await first.unmount();

		at('/admin');
		localStorage.setItem(STORAGE.adminView, 'people');
		await render(AdminHarness);
		await expect.element(page.getByRole('tab', { name: 'People' })).toHaveAttribute('aria-selected', 'true');
		await expect.element(region('Accounts')).toBeVisible();
	});

	it('at a glance: who watches, the disk, the trackers, the invitations, each leading to its view', async () => {
		household();
		await render(AdminHarness);
		const glance = page.getByRole('navigation', { name: 'At a glance' });
		await expect.element(glance.getByRole('link', { name: '3 people are watching' })).toBeVisible();
		await expect.element(glance.getByRole('link', { name: '390 GB free on disk' })).toBeVisible();
		await expect.element(glance.getByRole('link', { name: '2 invitations waiting' })).toBeVisible();
		await glance.getByRole('link', { name: '7 of 9 trackers on, 1 failing' }).click();
		await expect.element(page.getByRole('tab', { name: 'System' })).toHaveAttribute('aria-selected', 'true');
		await expect.element(page.getByRole('heading', { name: 'Trackers' })).toHaveFocus();
	});

	it('now watching: what plays, where as clocks, the state in words, the app and its version, since when', async () => {
		household();
		await render(AdminHarness);
		const now = region('Now watching');
		await expect.element(now.getByRole('link', { name: 'Severance' })).toBeVisible();
		await expect.element(now.getByText('S2:E4 · Woe’s Hollow')).toBeVisible();
		await expect.element(now.getByText('42:14 of 52:00')).toBeVisible();
		await expect.element(now.getByText('10 min left')).toBeVisible();
		await expect.element(now.getByText('Movie · 2024')).toBeVisible();
		await expect.element(now.getByText('1:02:14 of 2:46:00')).toBeVisible();
		for (const state of ['Playing', 'Paused', 'Buffering']) await expect.element(now.getByText(state, { exact: true })).toBeVisible();
		await expect.element(now.getByText(`Web ${IRIS_WEB_VERSION} · Firefox · macOS`)).toBeVisible();
		await expect.element(now.getByText('Android TV 1.3.0')).toBeVisible();
		await expect.element(now.getByText(`Older than ${IRIS_WEB_VERSION}, the current release`)).toHaveLength(1);
		await expect.element(now.getByText(/^Since \d\d:\d\d, .* ago$/).first()).toBeVisible();
		await expect.element(now.getByRole('link', { name: 'Alex' })).toHaveAttribute('href', '/admin/users/u2/history');
		expect(now.element().textContent).not.toContain('Severance.S02');
		await now.getByRole('button', { name: 'The file' }).first().click();
		await expect.element(now.getByText(/Severance\.S02E04/)).toBeVisible();
	});

	it('a session silent for a while says so, and since when', async () => {
		const quiet = sessions.map((x, i) =>
			i === 0 ? Object.assign(structuredClone(x), { last_seen_at: new Date(Date.now() - 35_000).toISOString() }) : x
		);
		household({ 'GET /admin/active-sessions': quiet });
		await render(AdminHarness);
		await expect.element(region('Now watching').getByText(/No news.for 3\d s/)).toBeVisible();
	});

	it('the watch history: titles not release names, a page at a time, the next page on a press', async () => {
		const api = household();
		await render(AdminHarness);
		const history = region('Watch history');
		await expect.poll(() => playRows().length).toBe(20);
		await expect.element(history.getByRole('heading', { level: 3, name: 'Today' })).toBeVisible();
		const text = history.element().textContent ?? '';
		expect(text).not.toMatch(/1080p|2160p|\.mkv/);
		expect(text).toContain('Home Movies Xmas (2019)');
		const first = history.getByRole('link', { name: plays[0].display_name }).first();
		await expect.element(first).toHaveAttribute('href', `/admin/users/${plays[0].user_id}/history`);

		await history.getByRole('button', { name: 'Show more plays' }).click();
		await expect.poll(() => playRows().length).toBe(40);
		expect(historyCalls(api).map((c) => c.path)).toContain('/admin/watch-history?limit=20&offset=20');
		await expect.element(history.getByRole('button', { name: 'Show more plays' })).toHaveFocus();
	});

	it('the watch history narrows to one person and one kind, asked of the server', async () => {
		const api = household();
		await render(AdminHarness);
		await expect.poll(() => playRows().length).toBe(20);
		await page.getByRole('radio', { name: 'Movies' }).click();
		await expect.poll(() => historyCalls(api).at(-1)?.path).toBe('/admin/watch-history?kind=movie&limit=20');
		await expect.poll(() => [...playRows()].every((r) => r.textContent?.includes('Movie'))).toBe(true);

		await page.getByRole('button', { name: /^Person/ }).click();
		await page.getByRole('option', { name: 'Tom' }).click();
		await expect.poll(() => historyCalls(api).at(-1)?.path).toBe('/admin/watch-history?user_id=u24&kind=movie&limit=20');
		await expect.element(region('Watch history').getByText('No plays match these filters.')).toBeVisible();
		await page.getByRole('button', { name: 'Clear the filters' }).click();
		await expect.poll(() => playRows().length).toBe(20);
	});

	it('people: the most active first, a search past ten, the rest on a press', async () => {
		household();
		at('/admin?view=people');
		await render(AdminHarness);
		const accounts = region('Accounts');
		await expect.element(accounts.getByRole('link', { name: plays[0].display_name })).toBeVisible();
		await expect.poll(() => accounts.element().querySelectorAll('.list-row').length).toBe(10);
		await expect.element(accounts.getByText(/· Last played .* · \d+ plays$/).first()).toBeVisible();
		await accounts.getByRole('button', { name: 'Show all 25 people' }).click();
		await expect.poll(() => accounts.element().querySelectorAll('.list-row').length).toBe(25);
		await expect.element(accounts.getByText(/Never played anything/).first()).toBeVisible();
		await page.getByLabelText('Find a person').fill('jade@');
		await expect.poll(() => accounts.element().querySelectorAll('.list-row').length).toBe(1);
		await expect.element(page.getByText('1 match of 25')).toBeVisible();
	});

	it('a person’s actions from their menu: delete after asking, never oneself', async () => {
		const api = household({ 'DELETE /admin/users/u2': noContent() });
		at('/admin?view=people');
		await render(AdminHarness);
		await page.getByRole('button', { name: 'Show all 25 people' }).click();
		await page.getByRole('button', { name: 'Actions for Léonard' }).click();
		await expect.element(page.getByRole('menuitem', { name: 'Rename' })).toBeVisible();
		await expect.element(page.getByRole('menuitem', { name: 'Delete the account' })).not.toBeInTheDocument();
		await userEvent.keyboard('{Escape}');

		await page.getByRole('button', { name: 'Actions for Alex' }).click();
		await page.getByRole('menuitem', { name: 'Delete the account' }).click();
		await page.getByRole('alertdialog', { name: 'Delete Alex’s account?' }).getByRole('button', { name: 'Delete the account' }).click();
		await expect.poll(() => api.sent('DELETE', '/admin/users/u2')).toHaveLength(1);
	});

	it('sets a new password from the menu: too short is said under the field', async () => {
		const api = household({ 'POST /admin/users/u2/password': noContent() });
		at('/admin?view=people');
		await render(AdminHarness);
		await page.getByLabelText('Find a person').fill('alex@');
		await page.getByRole('button', { name: 'Actions for Alex' }).click();
		await page.getByRole('menuitem', { name: 'Set a new password' }).click();
		const field = page.getByLabelText('New password', { exact: true });
		await field.fill('short');
		await page.getByRole('button', { name: 'Set the new password' }).click();
		await expect.element(field).toHaveAccessibleDescription('At least 8 characters. Use at least 8 characters.');
		await field.fill('long enough');
		await page.getByRole('button', { name: 'Set the new password' }).click();
		await expect.poll(() => api.sent('POST', '/admin/users/u2/password')[0]?.body).toEqual({ new_password: 'long enough' });
		await expect.element(page.getByRole('dialog')).not.toBeInTheDocument();
	});

	it('invitations: the waiting ones with their expiry in words; used and expired folded behind their count', async () => {
		const api = household({ 'DELETE /admin/invitations/i2': noContent() });
		at('/admin?view=people');
		await render(AdminHarness);
		const invites = region('Invitations');
		await expect.element(invites.getByText(/^Stops working tomorrow at /)).toBeVisible();
		await expect.element(invites.getByText('Stops working in 6 days')).toBeVisible();
		await expect.element(invites.getByText(/^Used .* by /).first()).not.toBeInTheDocument();
		await invites.getByRole('button', { name: '11 used or expired invitations' }).click();
		await expect.element(invites.getByText(/^Used .* by Manon$/)).toBeVisible();
		await expect.element(invites.getByText(/^Expired /).first()).toBeVisible();

		const revokes = invites.getByRole('button', { name: /^Revoke the invitation made/ });
		await expect.element(revokes).toHaveLength(2);
		await revokes.last().click();
		await page.getByRole('alertdialog', { name: 'Revoke this invitation?' }).getByRole('button', { name: 'Revoke the invitation' }).click();
		await expect
			.poll(() => api.sent('DELETE', '/admin/invitations/i1').length + api.sent('DELETE', '/admin/invitations/i2').length)
			.toBe(1);
	});

	it('creates an invitation: its link has the focus, ready to copy', async () => {
		const later = new Date(Date.now() + 3 * 86_400_000).toISOString();
		const api = household({ 'POST /admin/invitations': { id: 'i3', token: 'tok xyz', expires_at: later } });
		const write = vi.fn(async () => {});
		vi.spyOn(navigator.clipboard, 'writeText').mockImplementation(write);
		at('/admin?view=people');
		await render(AdminHarness);
		await page.getByRole('button', { name: 'Create an invitation link' }).click();
		const link = page.getByLabelText('Invitation link');
		await expect.element(link).toHaveValue(`${location.origin}/register?token=tok%20xyz`);
		await expect.element(link).toHaveFocus();
		expect(api.sent('POST', '/admin/invitations')).toHaveLength(1);
		await page.getByRole('button', { name: 'Copy the link' }).click();
		expect(write).toHaveBeenCalledWith(`${location.origin}/register?token=tok%20xyz`);
	});

	it('the disk in one bar and in words; the prepared copies a few at a time', async () => {
		household();
		at('/admin?view=system');
		await render(AdminHarness);
		const disk = region('Disk');
		await expect.element(disk.getByRole('meter', { name: /1\.6 TB of 2\.0 TB used \(81%\), 390 GB free/ })).toBeVisible();
		await expect.element(disk.getByText('Clean-up starts at 85% (1.7 TB) and frees space down to 70%.')).toBeVisible();
		const upkeep = region('Maintenance');
		await expect.poll(() => upkeep.element().querySelectorAll('.list-row').length).toBe(6);
		await expect.element(upkeep.getByText(/^Being prepared · Updated /)).toBeVisible();
		await upkeep.getByRole('button', { name: 'Show all 9 copies' }).click();
		await expect.poll(() => upkeep.element().querySelectorAll('.list-row').length).toBe(9);
	});

	it('trackers: each one on or off in words, how its last search went, a failing one said so', async () => {
		household();
		at('/admin?view=system');
		await render(AdminHarness);
		const list = region('Trackers');
		await expect.element(list.getByText('7 of 9 on, 1 failing')).toBeVisible();
		await expect.element(list.getByText(/^On · Last search .*: answered in 420 ms$/)).toBeVisible();
		await expect
			.element(
				list
					.getByText(/^On · tr4ker · Last search .*: failed after 8\.0 s: provider error: timed out after 8s$/)
					.or(list.getByText(/^On · Last search .*: failed after 8\.0 s/))
			)
			.toBeVisible();
		await expect.element(list.getByText('Failing')).toBeVisible();
		await expect.element(list.getByText('Off in providers.toml · unit3d')).toBeVisible();
		const tos = list.getByRole('switch', { name: 'tos' });
		await expect.element(tos).toHaveAttribute('aria-disabled', 'true');
		await expect.element(tos).toHaveAccessibleDescription('Disabled in providers.toml: only the config can turn it on.');
	});

	it('turns a tracker off: the row changes once the server says so', async () => {
		const answer = deferred();
		const api = household({ 'PUT /admin/providers/tr4ker': () => answer.promise });
		at('/admin?view=system');
		await render(AdminHarness);
		const tr4ker = region('Trackers').getByRole('switch', { name: 'tr4ker' });
		await expect.element(tr4ker).toBeChecked();
		await tr4ker.click();
		await expect.poll(() => api.sent('PUT', '/admin/providers/tr4ker')[0]?.body).toEqual({ enabled: false });
		await expect.element(tr4ker).toHaveAttribute('aria-busy', 'true');
		await expect.element(tr4ker).toBeChecked();

		const off = structuredClone(trackers);
		off[4].enabled = false;
		api.routes['GET /admin/providers'] = off;
		answer.resolve(off[4]);
		await expect.element(tr4ker).not.toBeChecked();
		await expect
			.element(
				region('Trackers')
					.getByText('Off · searches skip it · tr4ker')
					.or(region('Trackers').getByText(/^Off · searches skip it$/))
			)
			.toBeVisible();
	});

	it('frees disk space after asking, and says what went', async () => {
		const api = household({
			'POST /admin/gc': {
				used_bytes_before: 900,
				used_bytes_after: 400,
				target_bytes: 700,
				threshold_bytes: 850,
				derived_freed_bytes: 0,
				evicted: [{ infohash: 'aa', name: 'Old.Film.mkv', freed_bytes: 500 }]
			}
		});
		at('/admin?view=system');
		await render(AdminHarness);
		await page.getByRole('button', { name: 'Free space now' }).click();
		await page.getByRole('alertdialog', { name: 'Free space now?' }).getByRole('button', { name: 'Free space' }).click();
		await expect.poll(() => api.sent('POST', '/admin/gc')).toHaveLength(1);
		await expect.element(page.getByText('Old.Film.mkv')).toBeVisible();
	});

	it('the log: by day, in words, narrowed to a kind of action and a person, a page at a time', async () => {
		const api = household();
		at('/admin?view=log');
		await render(AdminHarness);
		const log = region('Audit log');
		await expect.element(log.getByRole('heading', { level: 3, name: 'Today' })).toBeVisible();
		await expect.element(log.getByText('4 releases removed, 45.0 GB freed').first()).toBeVisible();
		await expect.poll(() => log.element().querySelectorAll('.entry').length).toBe(20);
		await log.getByRole('button', { name: 'Show more entries' }).click();
		await expect.poll(() => log.element().querySelectorAll('.entry').length).toBe(40);

		await page.getByRole('button', { name: /^What/ }).click();
		await page.getByRole('option', { name: 'Accounts' }).click();
		await expect.poll(() => api.calls.at(-1)?.path).toBe('/admin/audit-log?action=user&limit=20');
		await page.getByRole('button', { name: /^Who/ }).click();
		await page.getByRole('option', { name: 'Alex' }).click();
		await expect.poll(() => api.calls.at(-1)?.path).toBe('/admin/audit-log?action=user&actor_id=u2&limit=20');
	});

	it('fits a 320 px screen in every view: nothing wider than it', async () => {
		household();
		await page.viewport(320, 900);
		for (const view of ['activity', 'people', 'system', 'log']) {
			at(`/admin?view=${view}`);
			const shown = await render(AdminHarness);
			await expect.poll(() => document.querySelectorAll('.group').length).toBeGreaterThan(0);
			await expect.poll(() => document.querySelectorAll('.list-row, li.play, .entry').length).toBeGreaterThan(0);
			expect(document.documentElement.scrollWidth, view).toBeLessThanOrEqual(320);
			await shown.unmount();
		}
		await page.viewport(414, 896);
	});

	it('every view passes axe', async () => {
		household();
		await render(AdminHarness);
		for (const name of ['Activity', 'People', 'System', 'Log']) {
			await page.getByRole('tab', { name }).click();
			await expect.poll(() => document.querySelectorAll('.list-row, li.play, .entry').length).toBeGreaterThan(0);
			const result = await axe.run(document.body);
			expect(result.violations.map((v) => `${name}: ${v.id}`)).toEqual([]);
		}
	});
});

describe('a person’s page', () => {
	beforeEach(() => session.signedIn({ ...leonard, id: people[0].id }));

	it('who they are, what plays now, and only their plays, without their name on each', async () => {
		const api = household();
		await render(AdminHarness, { person: 'u2' });
		await expect.element(page.getByRole('heading', { level: 1, name: 'Alex' })).toBeVisible();
		await expect.element(page.getByText(/^alex@example\.com · Joined .* · Last played /)).toBeVisible();
		await expect.element(page.getByText('Severance, S2:E4 · Woe’s Hollow')).toBeVisible();
		await expect.poll(() => document.querySelectorAll('li.play').length).toBeGreaterThan(0);
		expect(document.querySelector('li.play .who')).toBeNull();
		expect(historyCalls(api)[0]?.path).toBe('/admin/watch-history?user_id=u2&limit=20');
		await expect.element(page.getByRole('button', { name: /^Person/ })).not.toBeInTheDocument();
	});

	it('an account gone says so', async () => {
		household();
		await render(AdminHarness, { person: 'nobody' });
		await expect.element(page.getByText('This account no longer exists.')).toBeVisible();
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
