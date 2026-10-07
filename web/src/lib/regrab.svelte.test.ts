import { beforeEach, describe, expect, it, vi } from 'vitest';
import { json } from '#lib/test/fetch.ts';
import { stubApi } from '#lib/test/api.ts';
import { ui } from '#lib/ui.svelte.ts';
import { fetchAgainOrSearch, retrySearchQuery } from './regrab.ts';

const nav = vi.hoisted(() => ({ goto: vi.fn(async () => {}) }));
vi.mock('$app/navigation', () => nav);

const refused = (status: number, error: string, message: string) => json({ error, message }, status);
const severance = { infohash: 'h1', title: 'Severance', episode: { season: 2, episode: 4 }, name: 'Severance.S02E04.1080p.WEB.H265-X' };
const lastToast = () => ui.toasts.at(-1)?.text;

describe('a reclaimed release fetched again', () => {
	beforeEach(() => {
		nav.goto.mockClear();
		ui.toasts = [];
	});

	it('searches for the series and episode, else the title, never its SCENE name', () => {
		expect(retrySearchQuery('Severance', { season: 2, episode: 4 }, 'Severance.S02E04.1080p')).toBe('Severance S02E04');
		expect(retrySearchQuery('Dune', undefined, 'x')).toBe('Dune');
		expect(retrySearchQuery(null, undefined, 'Mercato.2025.FRENCH.1080p.WEB.H265-BOUBA.mkv')).toBe('Mercato (2025)');
	});

	it('comes back: the answer, no detour', async () => {
		stubApi({ 'POST /torrents/h1/regrab': { infohash: 'h1', name: 'x' } }, null);
		expect(await fetchAgainOrSearch(severance)).toMatchObject({ infohash: 'h1' });
		expect(nav.goto).not.toHaveBeenCalled();
		expect(ui.toasts).toHaveLength(0);
	});

	it.each([
		['its tracker is off', refused(409, 'provider_off', 'This tracker is turned off in Admin.'), 'This tracker is turned off in Admin.'],
		[
			'nobody seeds it',
			refused(409, 'dead_torrent', "this release has no seeders and can't be downloaded"),
			'Nobody shares this release any more.'
		],
		['the tracker lost it', refused(404, 'not_found', 'not found'), 'Its tracker no longer has this release.'],
		[
			'the tracker cannot find it',
			refused(400, 'bad_request', 'provider: torrent 42 not found'),
			'Its tracker no longer has this release.'
		],
		['the tracker declines', refused(409, 'provider_refused', 'Download rights revoked.'), 'Download rights revoked.']
	])('refused (%s): Search with the episode asked, the refusal said', async (_, reply, words) => {
		stubApi({ 'POST /torrents/h1/regrab': reply }, null);
		expect(await fetchAgainOrSearch(severance)).toBeUndefined();
		expect(nav.goto).toHaveBeenCalledWith('/search?q=Severance+S02E04');
		expect(lastToast()).toBe(`${words} Here are other releases of Severance S02E04.`);
	});

	it('a release with no title is searched by its cleaned-up name', async () => {
		stubApi({ 'POST /torrents/m1/regrab': refused(409, 'provider_off', 'This tracker is turned off in Admin.') }, null);
		await fetchAgainOrSearch({ infohash: 'm1', name: 'Mercato.2025.FRENCH.1080p.WEB.H265-BOUBA' });
		expect(nav.goto).toHaveBeenCalledWith('/search?q=Mercato+%282025%29');
		expect(lastToast()).toBe('This tracker is turned off in Admin. Here are other releases of Mercato (2025).');
	});

	it('no answer at all, or a server failure, stays put and is thrown for the gesture to say', async () => {
		stubApi({ 'POST /torrents/h1/regrab': new TypeError('Failed to fetch') }, null);
		await expect(fetchAgainOrSearch(severance)).rejects.toThrow(TypeError);
		stubApi({ 'POST /torrents/h1/regrab': refused(500, 'internal', 'internal server error') }, null);
		await expect(fetchAgainOrSearch(severance)).rejects.toMatchObject({ status: 500 });
		expect(nav.goto).not.toHaveBeenCalled();
		expect(ui.toasts).toHaveLength(0);
	});
});
