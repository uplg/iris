import { describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { page } from 'vitest/browser';
import { stubApi } from '#lib/test/api.ts';
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
});
