import { describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { page } from 'vitest/browser';
import { stubApi } from '#lib/test/api.ts';
import { json } from '#lib/test/fetch.ts';
import { ui } from '#lib/ui.svelte.ts';
import '../../styles/app.css';
import HistoryPage from './HistoryPage.svelte';

const nav = vi.hoisted(() => ({ goto: vi.fn() }));
vi.mock('$app/navigation', () => nav);

const gone = {
	infohash: 'm1',
	file_idx: 2,
	torrent_name: 'Dune',
	completed: false,
	deleted: true,
	last_watched_at: new Date().toISOString(),
	position_seconds: 600,
	duration_seconds: 6000,
	tmdb_id: 7,
	tmdb_verified: true,
	source_provider: 'c411',
	source_external_id: '42'
};

describe('history page', () => {
	it('says when nothing was watched', async () => {
		stubApi({ 'GET /me/history?limit=200': [] });
		await render(HistoryPage);
		await expect.element(page.getByRole('heading', { level: 1, name: 'Watch history' })).toBeVisible();
		await expect.element(page.getByText('Nothing watched yet.')).toBeVisible();
	});

	it('downloads a gone release again, then plays it where it stopped', async () => {
		const api = stubApi({ 'GET /me/history?limit=200': [gone], 'POST /torrents/m1/regrab': { infohash: 'm1' } }, { poster_path: null });
		await render(HistoryPage);
		await page.getByRole('button', { name: 'Download Dune again' }).click();
		await expect.poll(() => nav.goto.mock.calls.at(-1)?.[0]).toBe('/watch/m1/2');
		// from the provenance the server recorded: the same release, the same files
		expect(api.sent('POST', '/torrents/m1/regrab')).toHaveLength(1);
	});

	it('a refused download again lands on Search for other releases of the episode, the refusal said', async () => {
		nav.goto.mockClear();
		const episode = { ...gone, torrent_name: 'Severance.S02E04.1080p', collection_title: 'Severance', season: 2, episode: 4 };
		stubApi(
			{
				'GET /me/history?limit=200': [episode],
				'POST /torrents/m1/regrab': json({ error: 'provider_off', message: 'This tracker is turned off in Admin.' }, 409)
			},
			{ poster_path: null }
		);
		await render(HistoryPage);
		await page.getByRole('button', { name: /^Download .* again$/ }).click();
		await expect.poll(() => nav.goto.mock.calls.at(-1)?.[0]).toBe('/search?q=Severance+S02E04');
		expect(nav.goto).not.toHaveBeenCalledWith('/watch/m1/2');
		expect(ui.toasts.at(-1)?.text).toBe('This tracker is turned off in Admin. Here are other releases of Severance S02E04.');
	});

	it('no answer at all stays here and says so', async () => {
		nav.goto.mockClear();
		stubApi({ 'GET /me/history?limit=200': [gone], 'POST /torrents/m1/regrab': new TypeError('Failed to fetch') }, { poster_path: null });
		await render(HistoryPage);
		await page.getByRole('button', { name: 'Download Dune again' }).click();
		await expect.poll(() => ui.toasts.at(-1)?.text).toBe('Iris did not answer. Check your connection, then try again.');
		expect(nav.goto).not.toHaveBeenCalled();
	});
});
