import { beforeEach, describe, expect, it, vi } from 'vitest';
import { page as screen, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { focusManager } from '@tanstack/svelte-query';
import { queryClient } from '#lib/query.ts';
import { KEYS } from '#lib/queries.ts';
import { stubApi, type ApiCall } from '#lib/test/api.ts';
import { ui } from '#lib/ui.svelte.ts';
import { answer, release, severance } from './fixtures.ts';
import { SEARCH_VIEW_KEY } from './view.ts';
import SearchScreen from './SearchScreen.svelte';

const route = vi.hoisted(() => ({ url: new URL('http://iris.test/search') }));
const nav = vi.hoisted(() => ({ goto: vi.fn(), replaceState: vi.fn() }));
vi.mock('$app/state', () => ({ page: route }));
vi.mock('$app/navigation', () => nav);

const at = (path: string) => (route.url = new URL(path, 'http://iris.test'));

/** A backend answering the search, the titles and the recent searches; `search` decides per call. */
function backend(search: (c: ApiCall) => unknown = () => answer([release()]), recent: unknown[] = []) {
	return stubApi(
		{ 'GET /me/recent-searches': recent, 'POST /me/recent-searches': null, 'DELETE /me/recent-searches': null },
		(c: ApiCall) => {
			if (c.path.startsWith('/search/titles')) return [severance];
			if (c.path.startsWith('/search?')) return search(c);
			if (c.path.startsWith('/me/recent-searches')) return null;
			if (c.path.startsWith('/metadata/tmdb/search')) return [];
			return undefined;
		}
	);
}
const searches = (api: ReturnType<typeof backend>) => api.calls.filter((c) => c.path.startsWith('/search?'));

describe('SearchScreen', () => {
	beforeEach(() => {
		localStorage.clear();
		at('/search');
	});

	it('searches on submit only, says how many, and keeps the words in the address', async () => {
		const api = backend();
		const say = vi.spyOn(ui, 'say');
		await render(SearchScreen);
		const field = screen.getByRole('combobox', { name: 'Title, year or release name' });
		await userEvent.fill(field, 'severance');
		expect(searches(api)).toHaveLength(0);
		await screen.getByRole('button', { name: 'Search', exact: true }).click();
		await expect.element(screen.getByText('1 release from 1 tracker')).toBeVisible();
		expect(searches(api)[0].path).toBe('/search?q=severance&page=1&limit=25');
		expect(say).toHaveBeenCalledWith('1 release from 1 tracker');
		expect(nav.replaceState).toHaveBeenLastCalledWith('/search?q=severance', {});
		// the search is kept in the account's recent searches
		expect(api.sent('POST', '/me/recent-searches')[0].body).toEqual({ query: 'severance' });
	});

	it('a return to the tab does not ask every tracker again', async () => {
		at('/search?q=severance');
		const api = backend();
		await render(SearchScreen);
		await expect.element(screen.getByText('1 release', { exact: true })).toBeVisible();
		expect(searches(api)).toHaveLength(1);
		// back much later: the answer is old, still not asked again (the app's provider mounts the client)
		queryClient.mount();
		for (const q of queryClient.getQueryCache().findAll({ queryKey: KEYS.search })) q.setState({ dataUpdatedAt: 0 });
		focusManager.setFocused(false);
		focusManager.setFocused(true);
		await new Promise((resolve) => requestAnimationFrame(resolve));
		expect(searches(api)).toHaveLength(1);
		focusManager.setFocused(undefined);
		queryClient.unmount();
	});

	it('Titles first; the chosen view is remembered in this browser', async () => {
		at('/search?q=severance');
		backend();
		const first = await render(SearchScreen);
		await expect.element(screen.getByRole('radio', { name: 'Titles' })).toHaveAttribute('aria-checked', 'true');
		await expect.element(screen.getByRole('link', { name: 'Severance' })).toBeVisible();
		await expect.element(screen.getByText('1 release', { exact: true })).toBeVisible();
		await screen.getByRole('radio', { name: 'List' }).click();
		expect(localStorage.getItem(SEARCH_VIEW_KEY)).toBe('list');
		await expect.element(screen.getByRole('button', { name: 'Grab and play' })).toBeVisible();
		await first.unmount();

		await render(SearchScreen);
		await expect.element(screen.getByRole('radio', { name: 'List' })).toHaveAttribute('aria-checked', 'true');
	});

	it('a title leads to its releases only, in the list', async () => {
		at('/search?q=severance');
		backend();
		await render(SearchScreen);
		await expect.element(screen.getByRole('link', { name: 'Severance' })).toHaveAttribute('href', '/search?q=severance&title=95396');

		at('/search?q=severance&title=95396');
		const api = backend();
		await render(SearchScreen);
		await expect.element(screen.getByRole('heading', { name: 'Releases of Severance' })).toBeVisible();
		expect(searches(api)[0].path).toContain('tmdb_id=95396');
		await expect.element(screen.getByRole('button', { name: 'Grab and play' }).first()).toBeVisible();
		await expect.element(screen.getByRole('link', { name: 'All titles' })).toHaveAttribute('href', '/search?q=severance');
	});

	it('the kind is asked of the trackers; the audio filter counts and narrows what is loaded', async () => {
		at('/search?q=severance');
		localStorage.setItem(SEARCH_VIEW_KEY, 'list');
		const api = backend(() =>
			answer([release(), release({ external_id: '2', language_tag: 'fr', title: 'Severance.S02.FRENCH.1080p.WEB-GRP', seeders: 4 })])
		);
		await render(SearchScreen);
		const audio = screen.getByRole('group', { name: 'Audio' });
		await expect.element(audio.getByText(/French \(VF\)/)).toBeVisible();
		await audio.getByText(/French \(VF\)/).click();
		await expect.element(audio.getByRole('radio', { name: 'French (VF) 1' })).toBeChecked();
		await expect.element(screen.getByText('Severance.S02.FRENCH.1080p.WEB-GRP')).toBeVisible();
		await expect.element(screen.getByText('Severance.S02.MULTi.1080p.WEB.H265-GRP')).not.toBeInTheDocument();

		const type = screen.getByRole('group', { name: 'Type' });
		await type.getByText('Series', { exact: true }).click();
		await expect.element(type.getByRole('radio', { name: 'Series' })).toBeChecked();
		await vi.waitFor(() => expect(searches(api).at(-1)?.path).toContain('kind=tv'));
	});

	it('a release nobody seeds cannot be grabbed, and says why', async () => {
		at('/search?q=severance');
		localStorage.setItem(SEARCH_VIEW_KEY, 'list');
		const api = backend(() => answer([release({ seeders: 0 })]));
		await render(SearchScreen);
		const grab = screen.getByRole('button', { name: 'Grab and play' });
		await expect.element(grab).toHaveAttribute('aria-disabled', 'true');
		await expect.element(grab).toHaveAccessibleDescription('No seeders right now');
		await grab.click({ force: true });
		expect(api.calls.some((c) => c.path.startsWith('/torrents'))).toBe(false);
	});

	it('what is on disk plays from there; the library matches come first', async () => {
		at('/search?q=severance');
		localStorage.setItem(SEARCH_VIEW_KEY, 'list');
		backend(() =>
			answer([release({ already_in_library: true, library_infohash: 'abc', library_file_idx: 2 })], {
				library_matches: [
					{
						collection_id: 'c1',
						display_title: 'Severance',
						kind: 'tv',
						episode_count: 9,
						torrent_count: 1,
						is_anime: false,
						tmdb_id: 95396
					}
				]
			})
		);
		await render(SearchScreen);
		const lib = screen.getByRole('region', { name: 'In your library' });
		await expect.element(lib.getByText('Series · 9 episodes on disk')).toBeVisible();
		await expect.element(lib.getByRole('link', { name: 'Open: Severance' })).toHaveAttribute('href', '/collection/c1');
		await expect.element(screen.getByRole('link', { name: 'Play from disk' })).toHaveAttribute('href', '/watch/abc/2');
		await expect.element(screen.getByText('1 match in your library · 1 release from 1 tracker')).toBeVisible();
	});

	it('a tracker that did not answer is said in place, with a way to ask again', async () => {
		at('/search?q=severance');
		const api = backend(() =>
			answer([release()], {
				providers: [
					{ id: 'torr9', current_page: 1, limit: 25, total_count: 1, total_pages: 1 },
					{ id: 'c411', current_page: 1, limit: 25, error: 'timeout' }
				]
			})
		);
		await render(SearchScreen);
		await expect
			.element(screen.getByRole('alert').getByText('c411 did not answer, so its releases are missing from this list.'))
			.toBeVisible();
		await expect.element(screen.getByText('1 release from 1 tracker · c411 did not answer')).toBeVisible();
		await screen.getByRole('button', { name: 'Retry c411' }).click();
		await vi.waitFor(() => expect(searches(api)).toHaveLength(2));
	});

	it('more releases on demand, appended', async () => {
		at('/search?q=severance');
		localStorage.setItem(SEARCH_VIEW_KEY, 'list');
		const api = backend((c) =>
			c.path.includes('page=2')
				? answer([release({ external_id: '2', title: 'Severance.S01.1080p-GRP' })], {
						providers: [{ id: 'torr9', current_page: 2, limit: 25, total_count: 2, total_pages: 2 }]
					})
				: answer([release()], { providers: [{ id: 'torr9', current_page: 1, limit: 25, total_count: 2, total_pages: 2 }] })
		);
		await render(SearchScreen);
		await screen.getByRole('button', { name: 'Show 1 more release' }).click();
		await expect.element(screen.getByText('Severance.S01.1080p-GRP')).toBeVisible();
		await expect.element(screen.getByText('Severance.S02.MULTi.1080p.WEB.H265-GRP')).toBeVisible();
		expect(searches(api).map((c) => c.path.includes('page=2'))).toEqual([false, true]);
		// a long list of results: rows out of view are skipped by layout and paint
		const row = screen.getByRole('listitem').filter({ hasText: 'Severance.S01.1080p-GRP' }).element();
		expect(getComputedStyle(row).contentVisibility).toBe('auto');
	});

	it('recent searches: one press searches again, a cross forgets one, Clear forgets them all', async () => {
		const recent = [
			{ query: 'dune', searched_at: '2026-10-01T10:00:00Z' },
			{ query: 'severance', searched_at: '2026-10-01T09:00:00Z' }
		];
		const api = backend(undefined, recent);
		await render(SearchScreen);
		const box = screen.getByRole('region', { name: 'Recent' });
		await box.getByRole('button', { name: 'dune' }).click();
		await vi.waitFor(() => expect(searches(api)[0]?.path).toContain('q=dune'));
		await box.getByRole('button', { name: 'Forget “severance”' }).click();
		await vi.waitFor(() => expect(api.sent('DELETE', '/me/recent-searches?q=severance')).toHaveLength(1));
		await box.getByRole('button', { name: 'Clear' }).click();
		await vi.waitFor(() => expect(api.sent('DELETE', '/me/recent-searches')).toHaveLength(1));
	});
});
