import { describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { page } from 'vitest/browser';
import type { HistoryItem } from '@iris/api/client';
import { stubApi } from '#lib/test/api.ts';
import '../../styles/app.css';
import HistoryList from './HistoryList.svelte';

const item = (over: Partial<HistoryItem>): HistoryItem => ({
	infohash: 'aa',
	file_idx: 0,
	torrent_name: 'Release.2024.1080p',
	completed: false,
	deleted: false,
	last_watched_at: new Date().toISOString(),
	position_seconds: 1930,
	duration_seconds: 3300,
	tmdb_verified: true,
	...over
});

const items = [
	item({ infohash: 's1', collection_id: 'c1', collection_title: 'Severance', season: 2, episode: 4, tmdb_id: 1 }),
	item({ infohash: 's2', collection_id: 'c1', collection_title: 'Severance', season: 2, episode: 3, completed: true }),
	item({ infohash: 'm1', torrent_name: 'Dune', deleted: true, source_provider: 'c411', source_external_id: '42' })
];

describe('HistoryList', () => {
	it('a series under its title with its episodes; how far and when, in words', async () => {
		stubApi({}, { poster_path: null });
		await render(HistoryList, { items });
		await expect.element(page.getByRole('heading', { name: 'Severance' })).toBeVisible();
		await expect.element(page.getByRole('link', { name: 'Severance' })).toHaveAttribute('href', '/collection/c1');
		await expect.element(page.getByRole('link', { name: 'Play Severance, S2:E4' })).toHaveAttribute('href', '/watch/s1/0');
		await expect.element(page.getByText(/58% watched, stopped at 32:10 · Last watched today at/).first()).toBeVisible();
		await expect.element(page.getByText(/Watched to the end/)).toBeVisible();
	});

	it('what is gone from disk says so, and can be downloaded again', async () => {
		stubApi({}, { poster_path: null });
		const onrestore = vi.fn(async () => {});
		await render(HistoryList, { items, onrestore });
		await expect.element(page.getByText('Gone from disk')).toBeVisible();
		await expect.element(page.getByRole('link', { name: /Dune/ })).not.toBeInTheDocument();
		await page.getByRole('button', { name: 'Download Dune again' }).click();
		expect(onrestore).toHaveBeenCalledWith(items[2]);
	});

	it('read only: nothing to download again, titles not linked to collections', async () => {
		stubApi({}, { poster_path: null });
		await render(HistoryList, { items, collections: false });
		await expect.element(page.getByRole('button', { name: /again/ })).not.toBeInTheDocument();
		await expect.element(page.getByRole('link', { name: 'Severance' })).not.toBeInTheDocument();
	});
});
