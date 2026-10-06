import axe from 'axe-core';
import { beforeEach, describe, expect, it } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import '../../styles/app.css';
import { controlAt } from '#lib/test/hit.ts';
import { stubApi } from '#lib/test/api.ts';
import { ui } from '#lib/ui.svelte.ts';
import { collection, torrent } from './fixtures.ts';
import { LIBRARY_VIEW_KEY } from './model.ts';
import LibraryHarness from './LibraryHarness.svelte';

const MB = 1024 ** 2;
const COLLECTIONS = '/library?view=collections';
const TORRENTS = '/library?view=torrents';
const CW = '/me/continue-watching?include_grabbable=true';

const items = [
	collection(),
	collection({ id: 'm', display_title: 'Arrival', kind: 'movie', episode_count: 0, torrent_count: 1, poster_path: null }),
	collection({ id: 'a', display_title: 'Frieren', is_anime: true }),
	collection({ id: 'g', display_title: 'Dark', ghost: true })
];
const releases = [
	torrent(),
	torrent({
		id: 't-2',
		infohash: 'bbb',
		name: 'Frieren.S01E05.1080p.mkv',
		collection_id: 'a',
		finished: false,
		progress_pct: 42,
		progress_bytes: 420 * MB,
		total_size_bytes: 1000 * MB,
		download_speed_bps: 6.1 * MB,
		peers: 24,
		can_delete: false,
		added_by_name: 'Sam'
	}),
	torrent({ id: 't-3', infohash: 'ccc', name: 'Arrival.2016.1080p.mkv', collection_id: 'm', state: 'paused', source_provider: 'nyaa' })
];

function backend(over: Record<string, unknown> = {}) {
	return stubApi({
		[COLLECTIONS]: { view: 'collections', items },
		[TORRENTS]: { view: 'torrents', items: releases, total_uploaded_bytes: 3 * 1024 ** 4, total_downloaded_bytes: 2 * 1024 ** 4 },
		[CW]: [],
		...over
	});
}

beforeEach(() => localStorage.clear());

describe('Library', () => {
	it('one h1, the title counts under it, the titles view first', async () => {
		backend();
		await render(LibraryHarness);
		await expect.element(page.getByRole('heading', { level: 1, name: 'Library' })).toBeVisible();
		expect(document.querySelectorAll('h1')).toHaveLength(1);
		await expect.element(page.getByText('3 titles · 1 movie · 1 series · 1 anime')).toBeVisible();
		expect(document.querySelector('dl')).toBeNull();
		await expect.element(page.getByRole('button', { name: 'Titles' })).toHaveAttribute('aria-pressed', 'true');
		await expect.element(page.getByRole('link', { name: 'Severance' })).toBeVisible();
	});

	it('cards say their state in words; a ghost is labelled and can be hidden', async () => {
		const api = backend({ 'POST /me/gone/dismiss': new Response(null, { status: 204 }) });
		await render(LibraryHarness);
		await expect.element(page.getByText('19 episodes on disk')).toBeVisible();
		await expect.element(page.getByText('Downloading S1:E5 · 42%')).toBeVisible();
		await expect.element(page.getByText('No longer on disk', { exact: true })).toBeVisible();
		await page.getByRole('button', { name: 'Hide Dark from my library' }).click();
		await expect.poll(() => api.sent('POST', '/me/gone/dismiss').map((c) => c.body)).toEqual([{ collection_id: 'g' }]);
		await expect.poll(() => api.sent('GET', COLLECTIONS).length).toBeGreaterThan(1);
		await expect.poll(() => ui.polite).toContain('Dark is hidden from your library');
	});

	it('filters by type and state, sorts, and says how many it shows', async () => {
		backend();
		await render(LibraryHarness);
		const status = page.getByRole('status').filter({ hasText: 'titles' });
		await expect.element(status).toHaveTextContent('4 titles');
		await page.getByRole('group', { name: 'Type' }).getByText('Movies').click();
		await expect.element(status).toHaveTextContent('Showing 1 of 4 titles');
		await expect.element(page.getByRole('link', { name: 'Arrival' })).toBeVisible();
		await expect.element(page.getByRole('link', { name: 'Severance' })).not.toBeInTheDocument();
		await page.getByRole('group', { name: 'Type' }).getByText('All').click();
		await page.getByRole('group', { name: 'Show' }).getByText('Downloading (1)').click();
		await expect.element(status).toHaveTextContent('Showing 1 of 4 titles');
		await expect.element(page.getByRole('link', { name: 'Frieren' })).toBeVisible();
		await page.getByRole('button', { name: 'Clear filters' }).first().click();
		await expect.element(status).toHaveTextContent('4 titles');

		await page.getByRole('button', { name: /^Sort/ }).click();
		await page.getByRole('option', { name: 'Title A to Z' }).click();
		await expect
			.poll(() => [...document.querySelectorAll('.poster-grid .name')].map((a) => a.textContent))
			.toEqual(['Arrival', 'Dark', 'Frieren', 'Severance']);
	});

	it('the view is remembered in this browser', async () => {
		backend();
		const first = await render(LibraryHarness);
		await page.getByRole('button', { name: 'Downloads and seeding' }).click();
		await expect.element(page.getByRole('heading', { level: 2, name: 'Seeding' })).toBeVisible();
		expect(localStorage.getItem(LIBRARY_VIEW_KEY)).toBe('downloads');
		await first.unmount();
		await render(LibraryHarness);
		await expect.element(page.getByRole('button', { name: 'Downloads and seeding' })).toHaveAttribute('aria-pressed', 'true');
		await expect.element(page.getByRole('heading', { level: 2, name: 'Downloading' })).toBeVisible();
	});

	it('releases are grouped, their state in words, a policy pause with its reason', async () => {
		localStorage.setItem(LIBRARY_VIEW_KEY, 'downloads');
		backend();
		await render(LibraryHarness);
		const dl = page.getByRole('region', { name: 'Downloading' });
		await expect.element(dl.getByText('Downloading · 42% · 6.1 MB/s · 24 peers · done in about 2 min')).toBeVisible();
		await expect.element(dl.getByText('Frieren.S01E05.1080p', { exact: true })).toBeVisible();
		await expect.element(dl.getByRole('progressbar')).toHaveAttribute('aria-valuetext', '42%');
		const attention = page.getByRole('region', { name: 'Needs attention' });
		await expect.element(attention.getByText('Paused after download · nyaa releases never seed')).toBeVisible();
		await expect.element(page.getByRole('region', { name: 'Seeding' }).getByRole('link', { name: 'Severance' })).toBeVisible();
	});

	it('a title or a release opens from its poster; a release keeps its own buttons', async () => {
		backend();
		await render(LibraryHarness);
		const card = page.getByRole('link', { name: 'Arrival' });
		await expect.element(card).toBeVisible();
		expect(controlAt(card.element().closest('li')!.querySelector('.art'))).toBe(card.element());
		await page.getByRole('button', { name: 'Downloads and seeding' }).click();
		const seeding = page.getByRole('region', { name: 'Seeding' });
		const title = seeding.getByRole('link', { name: 'Severance' });
		await expect.element(title).toBeVisible();
		const row = title.element().closest('li')!;
		expect(controlAt(row.querySelector('.thumb'))).toBe(title.element());
		for (const control of row.querySelectorAll('.actions a, .actions button')) expect(controlAt(control)).toBe(control);
	});

	it('delete is not operable without the right, and says why', async () => {
		localStorage.setItem(LIBRARY_VIEW_KEY, 'downloads');
		const api = backend();
		await render(LibraryHarness);
		const del = page.getByRole('button', { name: 'Delete Frieren' });
		await expect.element(del).toHaveAttribute('aria-disabled', 'true');
		await expect.element(del).toHaveAccessibleDescription('Only an admin or Sam can delete this');
		(del.element() as HTMLElement).click();
		await expect.element(page.getByRole('alertdialog')).not.toBeInTheDocument();
		expect(api.sent('DELETE', '/torrents/bbb')).toHaveLength(0);
	});

	it('delete asks once, naming the files; Keep goes back; Delete sends, reads again, refocuses', async () => {
		localStorage.setItem(LIBRARY_VIEW_KEY, 'downloads');
		const api = backend({ 'DELETE /torrents/aaa': new Response(null, { status: 204 }) });
		await render(LibraryHarness);
		await page.getByRole('button', { name: 'Delete Severance' }).click();
		const dialog = page.getByRole('alertdialog', { name: 'Delete Severance?' });
		await expect.element(dialog).toHaveAccessibleDescription(/1 file \(Severance\.S02E01\.mkv\)/);
		await dialog.getByRole('button', { name: 'Keep' }).click();
		expect(api.sent('DELETE', '/torrents/aaa')).toHaveLength(0);

		const before = api.sent('GET', TORRENTS).length;
		api.routes[TORRENTS] = { view: 'torrents', items: releases.slice(1), total_uploaded_bytes: 0 };
		await page.getByRole('button', { name: 'Delete Severance' }).click();
		await page.getByRole('alertdialog').getByRole('button', { name: 'Delete' }).click();
		await expect.poll(() => api.sent('DELETE', '/torrents/aaa')).toHaveLength(1);
		await expect.poll(() => api.sent('GET', TORRENTS).length).toBeGreaterThan(before);
		await expect.element(page.getByRole('region', { name: 'Seeding' })).not.toBeInTheDocument();
		await expect.poll(() => ui.polite).toContain('Deleted Severance');
		await expect.poll(() => document.activeElement?.textContent).toBe('Downloading');
	});

	it('a failed delete is said, and the row stays', async () => {
		localStorage.setItem(LIBRARY_VIEW_KEY, 'downloads');
		backend({
			'DELETE /torrents/aaa': new Response(JSON.stringify({ error: 'forbidden' }), {
				status: 403,
				headers: { 'Content-Type': 'application/json' }
			})
		});
		await render(LibraryHarness);
		await page.getByRole('button', { name: 'Delete Severance' }).click();
		await page.getByRole('alertdialog').getByRole('button', { name: 'Delete' }).click();
		await expect.poll(() => ui.assertive).toContain('Only an admin, or the person who added it, can do this.');
		await expect.element(page.getByRole('button', { name: 'Delete Severance' })).toBeVisible();
	});

	it('both views pass axe', async () => {
		backend();
		await render(LibraryHarness);
		await expect.element(page.getByRole('link', { name: 'Severance' })).toBeVisible();
		const titles = await axe.run(document.body);
		expect(titles.violations.map((v) => v.id)).toEqual([]);
		await page.getByRole('button', { name: 'Downloads and seeding' }).click();
		await expect.element(page.getByRole('heading', { level: 2, name: 'Seeding' })).toBeVisible();
		const downloads = await axe.run(document.body);
		expect(downloads.violations.map((v) => v.id)).toEqual([]);
	});

	it('fits a 320 px screen in both views', async () => {
		backend();
		await page.viewport(320, 2400);
		await render(LibraryHarness);
		await expect.element(page.getByRole('link', { name: 'Severance' })).toBeVisible();
		expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(320);
		await page.getByRole('button', { name: 'Downloads and seeding' }).click();
		await expect.element(page.getByRole('heading', { level: 2, name: 'Seeding' })).toBeVisible();
		expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(320);
	});

	it('filters releases by name or who added them', async () => {
		localStorage.setItem(LIBRARY_VIEW_KEY, 'downloads');
		backend();
		await render(LibraryHarness);
		await page.getByRole('searchbox', { name: 'Find a release' }).fill('sam');
		await expect.element(page.getByRole('status').filter({ hasText: 'releases' })).toHaveTextContent('Showing 1 of 3 releases');
		await expect.element(page.getByRole('region', { name: 'Seeding' })).not.toBeInTheDocument();
	});
});
