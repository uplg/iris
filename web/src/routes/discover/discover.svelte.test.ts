import { beforeEach, describe, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { stubApi } from '#lib/test/api.ts';
import Provided from '#lib/home/test/Provided.svelte';
import Discover from './+page.svelte';

// the current address, as SvelteKit would say it
const route = vi.hoisted(() => ({ url: new URL('http://iris.test/discover'), state: {} }));
vi.mock('$app/state', () => ({ page: route }));
const nav = vi.hoisted(() => ({ afterNavigate: vi.fn(), replaceState: vi.fn(), goto: vi.fn() }));
vi.mock('$app/navigation', () => nav);

const at = (path: string) => (route.url = new URL(path, 'http://iris.test'));

const card = (catalog_id: string, title: string, kind = 'movie') => ({
	catalog_id,
	title,
	kind,
	availability: 'available',
	is_anime: false,
	already_in_library: false,
	poster_url: `https://image.tmdb.org/t/p/w342/${catalog_id}.jpg`
});

function backend(routes: Record<string, unknown> = {}) {
	return stubApi({
		'/me/moods?kind=movie': { moods: [{ id: 'chills', label: 'Chills', featured_title: 'Alien', backdrop_url: null }] },
		'/me/moods?kind=tv': { moods: [{ id: 'cozy', label: 'Cozy', featured_title: null }] },
		'/me/for-you/page': { shelves: [{ key: 'trending', title: 'Trending this week', items: [card('k1', 'Arcane', 'tv')] }] },
		...routes
	});
}

const show = () => render(Provided, { props: { view: Discover } });

describe('discover', () => {
	beforeEach(() => {
		at('/discover');
		nav.replaceState.mockClear();
	});

	it('one h1, the mood board as links, the suggestion shelves, and where the trends come from', async () => {
		backend();
		await show();
		await expect.element(page.getByRole('heading', { level: 1, name: 'Discover' })).toBeVisible();
		await expect.poll(() => document.title).toBe('Discover · Iris');
		await expect.element(page.getByRole('link', { name: /Chills/ })).toHaveAttribute('href', '/discover?mood=chills&kind=movie');
		await expect.element(page.getByText('Now: Alien')).toBeVisible();
		await expect.element(page.getByRole('region', { name: 'Trending this week' }).getByRole('link', { name: 'Arcane' })).toBeVisible();
		await expect.element(page.getByRole('link', { name: /TMDB/ })).toHaveAttribute('href', 'https://www.themoviedb.org');
	});

	it('switches to series in place: the board for series, the address rewritten, the choice said', async () => {
		const api = backend();
		await show();
		const series = page.getByRole('radio', { name: 'Series' });
		await expect.element(page.getByRole('radio', { name: 'Movies' })).toHaveAttribute('aria-checked', 'true');
		await series.click();
		await expect.element(series).toHaveAttribute('aria-checked', 'true');
		await expect.element(page.getByRole('link', { name: 'Cozy' })).toHaveAttribute('href', '/discover?mood=cozy&kind=tv');
		expect(api.sent('GET', '/me/moods?kind=tv')).toHaveLength(1);
		expect(String(nav.replaceState.mock.calls.at(-1)?.[0])).toBe('http://iris.test/discover?kind=tv');
	});

	it('a mood’s titles: named from the board, a poster grid, a way back to the moods', async () => {
		at('/discover?mood=chills&kind=movie');
		backend({ '/me/moods/chills?kind=movie': { mood: 'chills', kind: 'movie', items: [card('m1', 'Alien'), card('m2', 'The Thing')] } });
		await show();
		const results = page.getByRole('region', { name: 'Chills' });
		await expect.element(results.getByText('2 movies')).toBeVisible();
		await expect.element(results.getByRole('link', { name: 'The Thing' })).toHaveAttribute('href', '/search?q=The%20Thing');
		await expect.element(results.getByRole('link', { name: 'All moods' })).toHaveAttribute('href', '/discover?kind=movie');
		expect(document.querySelectorAll('ul.poster-grid > li')).toHaveLength(2);
	});

	it('a mood with nothing to get says so', async () => {
		at('/discover?mood=chills&kind=movie');
		backend({ '/me/moods/chills?kind=movie': { mood: 'chills', kind: 'movie', items: [] } });
		await show();
		await expect.element(page.getByText('Nothing to get for this mood right now.')).toBeVisible();
	});

	it('"Not interested" goes to the server, then the results are read again', async () => {
		at('/discover?mood=chills&kind=movie');
		let items = [card('m1', 'Alien'), card('m2', 'The Thing')];
		const api = backend({
			'/me/moods/chills?kind=movie': () => ({ mood: 'chills', kind: 'movie', items }),
			'POST /me/for-you/dismiss': () => {
				items = items.slice(1);
				return new Response(null, { status: 204 });
			}
		});
		await show();
		await page.getByRole('button', { name: 'Not interested in Alien' }).click();
		await expect.element(page.getByRole('region', { name: 'Chills' }).getByText('1 movie', { exact: true })).toBeVisible();
		expect(api.sent('POST', '/me/for-you/dismiss')[0].body).toEqual({ catalog_id: 'm1' });
	});

	it('no suggestion yet: why, in words', async () => {
		backend({ '/me/for-you/page': { shelves: [] } });
		await show();
		await expect.element(page.getByText('Nothing suggested yet.')).toBeVisible();
	});
});
