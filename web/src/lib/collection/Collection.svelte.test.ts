import { beforeEach, describe, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import axe from 'axe-core';
import type { CollectionDetail, TorrentView } from '@iris/api/client';
import { stubApi } from '#lib/test/api.ts';
import Collection from './Collection.svelte';
// the page as drawn: the 320 px check needs the shared classes (.btn, .chip, tabs)
import '../../styles/app.css';

const nav = vi.hoisted(() => ({ goto: vi.fn(async () => {}) }));
vi.mock('$app/navigation', () => nav);

const torrent = (infohash: string, over: Partial<TorrentView> = {}): TorrentView => ({
	id: `id-${infohash}`,
	infohash,
	name: `Severance.S02.1080p.WEB.H265-${infohash}`,
	added_at: '2026-10-01T10:00:00Z',
	added_by: 'u2',
	added_by_name: 'Camille',
	can_delete: false,
	tmdb_verified: true,
	uploaded_bytes_total: 0,
	download_speed_bps: 0,
	fetched_at: '2026-10-06T10:00:00Z',
	files: [{ index: 0, path: 'Severance.S02E01.mkv', size_bytes: 1_000_000_000 }],
	finished: true,
	peers: 3,
	progress_bytes: 1_000_000_000,
	progress_pct: 100,
	state: 'live',
	total_size_bytes: 1_000_000_000,
	upload_speed_bps: 0,
	uploaded_bytes: 0,
	...over
});

const ep = (season: number, episode: number, infohash: string, file_idx: number, watched = false) => ({
	season,
	episode,
	infohash,
	file_idx,
	watched,
	language: 'english'
});

const offer = (season: number, episode: number, language: string, id: string) => ({
	season,
	episode,
	language,
	found_at: '2026-10-05T00:00:00Z',
	indexer_provider: 'c411',
	indexer_torrent_id: id,
	quality: '1080p',
	seeders: 40,
	size_bytes: 2_000_000_000
});

function series(over: Partial<CollectionDetail> = {}): CollectionDetail {
	return {
		id: 'c1',
		kind: 'tv',
		display_title: 'Severance',
		tmdb_id: 95396,
		numbering: 'seasonal',
		on_watchlist: true,
		normalized_name: 'severance',
		poster_path: '/poster.jpg',
		backdrop_path: '/backdrop.jpg',
		torrents: [
			torrent('t1', { name: 'Severance.S01.1080p.WEB.H265-ONE' }),
			torrent('t2', { can_delete: true }),
			torrent('t3', {
				name: 'Severance.S02E04.1080p.WEB.H265-THREE',
				finished: false,
				progress_pct: 42,
				progress_bytes: 420_000_000,
				download_speed_bps: 1_611_112
			})
		],
		episodes: [
			ep(1, 1, 't1', 0, true),
			ep(1, 2, 't1', 1, true),
			ep(2, 1, 't2', 0),
			ep(2, 2, 't2', 1),
			ep(2, 3, 't2', 2),
			ep(2, 4, 't3', 0)
		],
		available_episodes: [offer(2, 5, 'english', 'o1'), offer(2, 5, 'french', 'o2')],
		season_packs: [],
		gone_episodes: [],
		gone_releases: [],
		...over
	};
}

function backend(c: CollectionDetail = series(), extra: Parameters<typeof stubApi>[0] = {}) {
	return stubApi({
		...extra,
		[`GET /library/collections/${c.id}`]: () => c,
		'/torrents/t1/progress': [],
		'/torrents/t3/progress': [],
		'/torrents/t2/progress': [
			{ file_idx: 0, completed: false, position_seconds: 0, duration_seconds: 3120, last_watched_at: '2026-10-04T20:00:00Z' },
			{ file_idx: 1, completed: false, position_seconds: 1800, duration_seconds: 3180, last_watched_at: '2026-10-05T20:00:00Z' },
			{ file_idx: 2, completed: true, position_seconds: 3300, duration_seconds: 3300, last_watched_at: '2026-10-03T20:00:00Z' }
		],
		'/me/continue-watching?include_grabbable=true': [
			{
				infohash: 't2',
				file_idx: 1,
				season: 2,
				episode: 2,
				completed: false,
				grabbable: false,
				next_up: false,
				position_seconds: 1930,
				last_watched_at: '2026-10-05T20:00:00Z',
				tmdb_verified: true,
				torrent_name: 'x'
			}
		],
		'/metadata/tmdb/95396?kind=tv': {
			tmdb_id: 95396,
			kind: 'tv',
			title: 'Severance',
			year: 2022,
			genres: ['Drama', 'Mystery'],
			genre_ids: [],
			overview: 'Mark leads a team whose memories have been split.',
			vote_score: 0.84,
			original_language: 'en'
		},
		'/me/watchlist': [{ id: 'c1', name: 'Severance', normalized_name: 'severance', new_count: 0, created_at: '2026-01-01T00:00:00Z' }],
		'/me/playback-preferences?collection_id=c1': { audio_language: 'en', subtitle_language: 'off', for_collection: false },
		'PUT /me/playback-preferences': new Response(null, { status: 204 }),
		'POST /library/collections/c1/grab/2/5?language=english': { already_grabbed: false, infohash: 't9', file_idx: 3 },
		'DELETE /torrents/t2': new Response(null, { status: 204 })
	});
}

const reads = (api: ReturnType<typeof stubApi>, id = 'c1') => api.sent('GET', `/library/collections/${id}`).length;

describe('Collection', () => {
	beforeEach(() => nav.goto.mockClear());

	it('heads the page with the title, its facts in words and the way to resume', async () => {
		backend();
		await render(Collection, { id: 'c1' });
		await expect.element(page.getByRole('heading', { level: 1, name: 'Severance' })).toBeVisible();
		await expect.element(page.getByText('Series · 2022 · Drama, Mystery')).toBeVisible();
		await expect.element(page.getByText('2 seasons · 7 episodes · TMDB 8.4 · 3 releases on disk')).toBeVisible();
		await expect.element(page.getByText('English audio (original)')).toBeVisible();
		await expect.element(page.getByRole('listitem').filter({ hasText: /^1080p · HEVC$/ })).toBeVisible();
		await expect.element(page.getByRole('link', { name: 'Resume S2:E2 at 32:10' })).toHaveAttribute('href', '/watch/t2/1');
		await expect.element(page.getByRole('button', { name: 'On your watchlist' })).toHaveAttribute('aria-pressed', 'true');
		await expect.element(page.getByRole('navigation', { name: 'Breadcrumb' }).getByRole('link', { name: 'Library' })).toBeVisible();
	});

	it('marks the whole title watched, then reads it again', async () => {
		const api = backend(series(), { 'POST /library/collections/c1/watched': null });
		await render(Collection, { id: 'c1' });
		const watched = page.getByRole('button', { name: 'Watched' });
		await expect.element(watched).toHaveAttribute('aria-pressed', 'false');
		const before = reads(api);
		await watched.click();
		await vi.waitFor(() => expect(api.sent('POST', '/library/collections/c1/watched')).toHaveLength(1));
		await vi.waitFor(() => expect(reads(api)).toBeGreaterThan(before));
	});

	it('names each season by what is left, and says each episode’s state in words', async () => {
		backend();
		await render(Collection, { id: 'c1' });
		const s1 = page.getByRole('tab', { name: 'Season 1 · watched' });
		await expect.element(s1).toHaveAttribute('aria-selected', 'true');
		await page.getByRole('tab', { name: 'Season 2 · 5 episodes' }).click();
		const panel = page.getByRole('tabpanel', { name: 'Season 2 · 5 episodes' });
		await expect.element(panel.getByText('On disk · 52 min')).toBeVisible();
		await expect.element(panel.getByText('In progress · 23 min left')).toBeVisible();
		await expect.element(panel.getByText('Watched · 55 min')).toBeVisible();
		await expect.element(panel.getByText('Downloading · 42% · done in about 6 min')).toBeVisible();
		await expect.element(panel.getByText('Available · 2 releases · English, French audio')).toBeVisible();
		await expect.element(panel.getByRole('link', { name: 'Resume: season 2, episode 2' })).toHaveAttribute('href', '/watch/t2/1');
		await expect.element(panel.getByRole('link', { name: 'Play while downloading: season 2, episode 4' })).toBeVisible();
	});

	it('grabs an episode in the chosen language, reads the collection again, then plays it', async () => {
		const api = backend();
		await render(Collection, { id: 'c1' });
		await page.getByRole('tab', { name: 'Season 2 · 5 episodes' }).click();
		const before = reads(api);
		await page.getByRole('button', { name: 'Grab and play in English: season 2, episode 5' }).click();
		await vi.waitFor(() => expect(nav.goto).toHaveBeenCalledWith('/watch/t9/3'));
		expect(api.sent('POST', '/library/collections/c1/grab/2/5?language=english')).toHaveLength(1);
		const grabAt = api.calls.findIndex((c) => c.method === 'POST');
		expect(api.calls.slice(grabAt).some((c) => c.path === '/library/collections/c1')).toBe(true);
		expect(reads(api)).toBeGreaterThan(before);
	});

	it('shows a delete it cannot do as not operable, with the reason beside it', async () => {
		backend();
		await render(Collection, { id: 'c1' });
		const locked = page.getByRole('button', { name: 'Delete Severance.S01.1080p.WEB.H265-ONE' });
		await expect.element(locked).toHaveAttribute('aria-disabled', 'true');
		await expect.element(locked).toHaveAccessibleDescription('Only an admin, or the person who added it, can delete this.');
	});

	it('deletes a release it may, after a confirmation naming the verb, then reads the collection again', async () => {
		const api = backend();
		await render(Collection, { id: 'c1' });
		const before = reads(api);
		await page.getByRole('button', { name: 'Delete Severance.S02.1080p.WEB.H265-t2' }).click();
		const dialog = page.getByRole('alertdialog', { name: 'Delete this release?' });
		await expect.element(dialog.getByRole('button', { name: 'Keep' })).toBeVisible();
		await dialog.getByRole('button', { name: 'Delete release' }).click();
		await vi.waitFor(() => expect(api.sent('DELETE', '/torrents/t2')).toHaveLength(1));
		await vi.waitFor(() => expect(reads(api)).toBeGreaterThan(before));
	});

	it('saves this series’ languages with its collection id', async () => {
		const api = backend();
		await render(Collection, { id: 'c1' });
		const panel = page.getByRole('region', { name: 'Next episodes play with' });
		await expect.element(panel.getByText('Your usual choice, from your account.')).toBeVisible();
		await panel.getByRole('button', { name: 'Change languages' }).click();
		const sheet = page.getByRole('dialog', { name: 'Languages for Severance' });
		const audio = sheet.getByRole('group', { name: 'Audio' });
		await audio.getByText('French', { exact: true }).click();
		await expect.element(audio.getByRole('radio', { name: 'French' })).toBeChecked();
		await sheet.getByRole('button', { name: 'Save for this series' }).click();
		await vi.waitFor(() => expect(api.sent('PUT', '/me/playback-preferences')).toHaveLength(1));
		expect(api.sent('PUT', '/me/playback-preferences')[0].body).toEqual({
			audio_language: 'fr',
			subtitle_language: 'off',
			collection_id: 'c1'
		});
	});

	it('lays a fleuve anime out as one list on the absolute number, without seasons', async () => {
		backend(
			series({
				id: 'op',
				numbering: 'absolute',
				tmdb_id: null,
				torrents: [torrent('t1')],
				episodes: [
					{ ...ep(1, 1155, 't1', 0, true), absolute_episode: 1155 },
					{ ...ep(1, 1156, 't1', 1), absolute_episode: 1156 }
				],
				available_episodes: [{ ...offer(1, 1157, 'english', 'o1'), absolute_episode: 1157 }, offer(23, 7, 'english', 'cut')]
			})
		);
		await render(Collection, { id: 'op' });
		const list = page.getByRole('list', { name: '3 episodes' });
		await expect.element(list).toBeVisible();
		await expect.element(list.getByRole('heading', { name: 'Episode 1157' })).toBeVisible();
		await expect.element(page.getByRole('tablist')).not.toBeInTheDocument();
		await expect.element(page.getByText('Episode 7', { exact: true })).not.toBeInTheDocument();
	});

	it('fits a 320 px screen and passes axe', async () => {
		backend();
		await page.viewport(320, 800);
		const { container } = await render(Collection, { id: 'c1' });
		await expect.element(page.getByRole('heading', { level: 1, name: 'Severance' })).toBeVisible();
		await expect.element(page.getByText('Watched', { exact: false }).first()).toBeVisible();
		expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(320);
		const result = await axe.run(container, { rules: { 'color-contrast': { enabled: false } } });
		expect(result.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`)).toEqual([]);
	});

	it('takes a movie with a single copy straight to the player', async () => {
		backend(
			series({
				id: 'm1',
				kind: 'movie',
				tmdb_id: null,
				torrents: [torrent('mv', { files: [{ index: 2, path: 'Dune.2021.mkv', size_bytes: 9 }] })],
				episodes: [],
				available_episodes: []
			})
		);
		await render(Collection, { id: 'm1' });
		await vi.waitFor(() => expect(nav.goto).toHaveBeenCalledWith('/watch/mv/2', { replaceState: true }));
	});
});
