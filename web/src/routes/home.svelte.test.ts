import { beforeEach, describe, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import type { CollectionListItem, ContinueWatchingItem, WatchlistItem } from '@iris/api/client';
import '../styles/app.css';
import { stubApi } from '#lib/test/api.ts';
import { ui } from '#lib/ui.svelte.ts';
import Provided from '#lib/home/test/Provided.svelte';
import Home from './+page.svelte';

const nav = vi.hoisted(() => ({ goto: vi.fn(async (_to: string) => {}) }));
vi.mock('$app/navigation', () => nav);

const tile = (over: Partial<ContinueWatchingItem> = {}): ContinueWatchingItem => ({
	collection_id: 'c1',
	completed: false,
	duration_seconds: 3300,
	episode: 4,
	file_idx: 3,
	grabbable: false,
	infohash: 'abc',
	kind: 'tv',
	last_watched_at: '2026-10-05T20:00:00Z',
	next_up: false,
	position_seconds: 1930,
	season: 2,
	tmdb_id: 95396,
	tmdb_verified: true,
	torrent_name: 'Severance.S02E04.1080p.WEB.H264-GRP',
	...over
});

const followed = (over: Partial<WatchlistItem> = {}): WatchlistItem => ({
	id: 'c2',
	created_at: '2026-09-01T00:00:00Z',
	name: 'Andor',
	new_count: 0,
	normalized_name: 'andor',
	poster_path: '/andor.jpg',
	...over
});

const title = (over: Partial<CollectionListItem> = {}): CollectionListItem => ({
	id: 'c3',
	display_title: 'Dune',
	episode_count: 1,
	kind: 'movie',
	poster_path: '/dune.jpg',
	torrent_count: 1,
	total_size_bytes: 8 * 1024 ** 3,
	...over
});

const never = () => new Promise(() => {});

/** The home's backend: everything quiet unless a test says otherwise. */
function home(routes: Record<string, unknown> = {}) {
	return stubApi({
		'/me/continue-watching?include_grabbable=true': [],
		'/me/watchlist': [],
		'/me/for-you': { shelves: [] },
		'/library?view=collections': { view: 'collections', items: [] },
		'/library?view=torrents': { view: 'torrents', items: [] },
		'/me/summary': { downloading: 0, downloading_pct: 0, new_episodes: 0, seeding: 0 },
		'/me/preferences': { languages: [], genres: [], include_anime: false, onboarding_completed: true },
		'/discover/featured': { movies: [], series: [] },
		'/metadata/tmdb/95396?kind=tv': {
			title: 'Severance',
			backdrop_path: '/sev.jpg',
			overview: 'Work and life, apart.',
			genre_ids: [],
			genres: [],
			kind: 'tv',
			tmdb_id: 95396
		},
		'/me/playback-preferences?collection_id=c1': { audio_language: 'fr', subtitle_language: 'off' },
		...routes
	});
}

const show = () => render(Provided, { props: { view: Home } });
const shelf = (name: string) => page.getByRole('region', { name, exact: true });

describe('home', () => {
	beforeEach(() => nav.goto.mockClear());

	it('resumes the last thing: where it stopped, what is left, the languages it will play with', async () => {
		home({ '/me/continue-watching?include_grabbable=true': [tile()] });
		await show();
		const hero = page.getByRole('region', { name: 'Severance' });
		await expect.element(hero.getByText('Continue where you left off')).toBeVisible();
		await expect.element(hero.getByText('Season 2 · Episode 4 · 55 min')).toBeVisible();
		await expect.element(hero.getByText('23 min left')).toBeVisible();
		await expect.element(hero.getByRole('link', { name: 'Resume at 32:10' })).toHaveAttribute('href', '/watch/abc/3');
		await expect.element(hero.getByRole('link', { name: 'All episodes' })).toHaveAttribute('href', '/collection/c1');
		await expect.element(hero.getByRole('button', { name: 'Start over' })).toBeVisible();
		await expect.element(hero.getByText('Plays with audio in French, subtitles off.')).toBeVisible();
		await expect.element(shelf('Continue watching').getByText('S2:E4')).toBeVisible();
		await expect.poll(() => document.title).toBe('Iris');
	});

	it('gets a next episode that is not on disk, then plays it', async () => {
		const api = home({
			'/me/continue-watching?include_grabbable=true': [
				tile({ grabbable: true, infohash: '', next_up: true, episode: 5, position_seconds: 0 })
			],
			'POST /library/collections/c1/grab/2/5?language=auto': { already_grabbed: false, infohash: 'def', file_idx: 7 }
		});
		await show();
		await page.getByRole('region', { name: 'Severance' }).getByRole('button', { name: 'Play S2:E5' }).click();
		await expect.poll(() => nav.goto.mock.calls.at(-1)?.[0]).toBe('/watch/def/7');
		expect(api.sent('POST', '/library/collections/c1/grab/2/5?language=auto')).toHaveLength(1);
		await expect.element(shelf('Continue watching').getByText('Up next · S2:E5 · Not downloaded')).toBeVisible();
	});

	it('a failed get says why and leads to the series page', async () => {
		home({
			'/me/continue-watching?include_grabbable=true': [tile({ grabbable: true, infohash: '', next_up: true, episode: 5 })],
			'POST /library/collections/c1/grab/2/5?language=auto': new Response(
				JSON.stringify({ error: 'not_found', message: 'No release has seeders.' }),
				{
					status: 404,
					headers: { 'Content-Type': 'application/json' }
				}
			)
		});
		await show();
		await shelf('Continue watching').getByRole('button', { name: 'Get S2:E5 of Severance and play' }).click();
		await expect.poll(() => ui.toasts.at(-1)?.text).toBe('Could not get S2:E5. No release has seeders.');
		ui.toasts.at(-1)?.action?.run();
		expect(nav.goto).toHaveBeenLastCalledWith('/collection/c1');
	});

	it('removes a series from Continue watching through the server, then reads the row again', async () => {
		let rows = [
			tile(),
			tile({
				collection_id: null,
				kind: 'movie',
				infohash: 'mov',
				file_idx: 0,
				season: null,
				episode: null,
				tmdb_id: null,
				torrent_name: 'Dune.2021.2160p'
			})
		];
		const api = home({
			'/me/continue-watching?include_grabbable=true': () => rows,
			'POST /me/continue-watching/dismiss': () => {
				rows = rows.slice(1);
				return new Response(null, { status: 204 });
			}
		});
		await show();
		const row = shelf('Continue watching');
		await row.getByRole('button', { name: 'More for Severance' }).click();
		await page.getByRole('menuitem', { name: 'Remove from Continue watching' }).click();
		await expect.element(row.getByRole('link', { name: 'Severance' })).not.toBeInTheDocument();
		expect(api.sent('POST', '/me/continue-watching/dismiss')[0].body).toEqual({ collection_id: 'c1' });
		expect(api.sent('GET', '/me/continue-watching?include_grabbable=true').length).toBeGreaterThanOrEqual(2);
		await expect.element(row.getByRole('link', { name: 'Dune (2021)' })).toBeVisible();
		// the menu's page lock went with it: the page takes the pointer again
		await expect.poll(() => document.body.style.pointerEvents).toBe('');
	});

	it('a movie leaves the row by its file', async () => {
		const api = home({
			'/me/continue-watching?include_grabbable=true': [
				tile({ collection_id: null, kind: 'movie', infohash: 'mov', file_idx: 0, tmdb_id: null, torrent_name: 'Dune.2021.2160p' })
			],
			'POST /me/continue-watching/dismiss': new Response(null, { status: 204 })
		});
		await show();
		await shelf('Continue watching').getByRole('button', { name: 'More for Dune (2021)' }).click();
		await page.getByRole('menuitem', { name: 'Remove from Continue watching' }).click();
		await expect.poll(() => api.sent('POST', '/me/continue-watching/dismiss')[0]?.body).toEqual({ infohash: 'mov', file_idx: 0 });
	});

	it('fits a 320 px screen: no sideways scroll, the still under the words', async () => {
		await page.viewport(320, 640);
		home({ '/me/continue-watching?include_grabbable=true': [tile()] });
		const { container } = await show();
		await expect.element(page.getByRole('link', { name: 'Resume at 32:10' })).toBeVisible();
		const hero = container.querySelector('.hero');
		expect(hero?.scrollWidth).toBeLessThanOrEqual(hero?.clientWidth ?? 0);
		expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(320);
		await page.viewport(1280, 800);
	});

	it('says what Iris is doing right now, in words', async () => {
		home({
			'/me/summary': {
				downloading: 2,
				downloading_pct: 64,
				new_episodes: 3,
				seeding: 18,
				disk: { free_bytes: 412 * 1024 ** 3, total_bytes: 2000 * 1024 ** 3 }
			}
		});
		await show();
		const now = page.getByRole('region', { name: 'Right now' });
		for (const fact of ['2 downloads · 64%', '3 new episodes on your watchlist', '412 GB free on disk', 'Seeding 18 releases'])
			await expect.element(now.getByText(fact)).toBeVisible();
	});

	it('the watchlist: fresh episodes first, what downloads said in words', async () => {
		home({
			'/me/watchlist': [followed(), followed({ id: 'c4', name: 'The Bear', normalized_name: 'the bear', new_count: 3 })],
			'/library?view=torrents': {
				view: 'torrents',
				items: [{ collection_id: 'c2', finished: false, progress_bytes: 42, total_size_bytes: 100 }]
			}
		});
		await show();
		const row = shelf('Your watchlist');
		await expect.element(row.getByText('3 new episodes')).toBeVisible();
		await expect.element(row.getByText('Downloading · 42%')).toBeVisible();
		const names = [...row.element().querySelectorAll('.name')].map((a) => a.textContent);
		expect(names).toEqual(['The Bear', 'Andor']);
	});

	it('with nothing to resume, the library leads; its row says what is on disk', async () => {
		home({
			'/library?view=collections': { view: 'collections', items: [title(), title({ id: 'c5', display_title: 'Alien', ghost: true })] }
		});
		await show();
		await expect.element(page.getByRole('region', { name: 'Dune' }).getByText('In your library')).toBeVisible();
		const row = shelf('Your library');
		await expect.element(row.getByText('On disk')).toBeVisible();
		await expect.element(row.getByText('No longer on disk')).toBeVisible();
		await expect.element(row.getByRole('link', { name: 'See all' })).toHaveAttribute('href', '/library');
	});

	it('empty rows say why and what to do; a row that failed says so with a retry', async () => {
		home({
			'/me/watchlist': new Response(JSON.stringify({ error: 'boom', message: 'The database is busy.' }), {
				status: 400,
				headers: { 'Content-Type': 'application/json' }
			})
		});
		await show();
		await expect
			.element(shelf('Continue watching').getByText('Nothing to resume yet. What you start watching waits for you here.'))
			.toBeVisible();
		await expect.element(shelf('Your library').getByRole('link', { name: 'search' })).toHaveAttribute('href', '/search');
		const watch = shelf('Your watchlist');
		await expect.element(watch.getByText('This could not be loaded.')).toBeVisible();
		await expect.element(watch.getByText('The database is busy.')).toBeVisible();
		await expect.element(watch.getByRole('button', { name: 'Try again' })).toBeVisible();
	});

	it('suggestion shelves come after the library: a card leads to a search for its title; "Not interested" asks the server, then rereads', async () => {
		let shelves = [
			{
				key: 'trending',
				title: 'Trending this week',
				items: [
					{
						catalog_id: 'k1',
						title: 'Arcane',
						kind: 'tv',
						availability: 'available',
						is_anime: false,
						already_in_library: false,
						year: 2021,
						reason: '1.8k watching today'
					}
				]
			}
		];
		const api = home({
			'/me/for-you': () => ({ shelves }),
			'POST /me/for-you/dismiss': () => {
				shelves = [];
				return new Response(null, { status: 204 });
			}
		});
		await show();
		const row = shelf('Trending this week');
		await expect.element(row.getByRole('link', { name: 'Arcane' })).toHaveAttribute('href', '/search?q=Arcane');
		await expect.element(row.getByText('1.8k watching today')).toBeVisible();
		const order = [...document.querySelectorAll('.home h2')].map((h) => h.textContent);
		expect(order).toEqual(['Continue watching', 'Your watchlist', 'Your library', 'Trending this week']);
		await row.getByRole('button', { name: 'Not interested in Arcane' }).click();
		await expect.element(shelf('Trending this week')).not.toBeInTheDocument();
		expect(api.sent('POST', '/me/for-you/dismiss')[0].body).toEqual({ catalog_id: 'k1' });
	});

	it('first visit, skipped: what the server already holds is kept, only marked done', async () => {
		const api = home({
			'/me/preferences': { languages: ['english'], genres: [18], include_anime: true, onboarding_completed: false },
			'/languages': { languages: [{ value: 'english', label: 'English' }] },
			'/genres': { genres: [{ id: 18, name: 'Drama' }] },
			'PUT /me/preferences': (c: { body: unknown }) => ({ ...(c.body as object) }),
			'/me/continue-watching?include_grabbable=true': never
		});
		await show();
		const sheet = page.getByRole('dialog', { name: 'Personalize your home' });
		await sheet.getByRole('button', { name: 'Skip for now' }).click();
		await expect.element(sheet).not.toBeInTheDocument();
		expect(api.sent('PUT', '/me/preferences')[0].body).toEqual({
			languages: ['english'],
			genres: [18],
			include_anime: true,
			onboarding_completed: true
		});
	});

	it('first visit: asks what one likes, saves it, and closes on the server’s answer', async () => {
		const api = home({
			'/me/preferences': { languages: [], genres: [], include_anime: false, onboarding_completed: false },
			'/languages': { languages: [{ value: 'french', label: 'French' }] },
			'/genres': { genres: [{ id: 18, name: 'Drama' }] },
			'PUT /me/preferences': (c: { body: unknown }) => ({ ...(c.body as object) }),
			'/me/continue-watching?include_grabbable=true': never
		});
		await show();
		const sheet = page.getByRole('dialog', { name: 'Personalize your home' });
		await sheet.getByRole('button', { name: 'French' }).click();
		await expect.element(sheet.getByRole('button', { name: 'French' })).toHaveAttribute('aria-pressed', 'true');
		await sheet.getByRole('button', { name: 'Anime' }).click();
		await sheet.getByRole('button', { name: 'Save preferences' }).click();
		await expect.element(sheet).not.toBeInTheDocument();
		expect(api.sent('PUT', '/me/preferences')[0].body).toEqual({
			languages: ['french'],
			genres: [],
			include_anime: true,
			onboarding_completed: true
		});
	});
});
