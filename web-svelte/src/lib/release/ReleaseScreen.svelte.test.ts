import { beforeEach, describe, expect, it, vi } from 'vitest';
import { page as screen } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import type { TorrentDetails, TorrentPreview } from '@iris/api/client';
import { json } from '#lib/test/fetch.ts';
import { stubApi } from '#lib/test/api.ts';
import ReleaseScreen from './ReleaseScreen.svelte';

const nav = vi.hoisted(() => ({ goto: vi.fn() }));
vi.mock('$app/navigation', () => nav);

const GB = 1024 ** 3;

function preview(over: Partial<TorrentPreview> = {}): TorrentPreview {
	return {
		announce_urls: [],
		infohash: 'abc',
		name: 'Dune.2021.MULTi.1080p.WEB.H265-GRP',
		piece_count: 1,
		piece_length: 1,
		streamable: true,
		total_size_bytes: 8 * GB,
		files: [
			{ index: 0, path: 'Dune.2021.MULTi.1080p.WEB.H265-GRP.mkv', size_bytes: 8 * GB, is_video: true, is_archive: false, extension: 'mkv' }
		],
		...over
	};
}

function details(over: Partial<TorrentDetails> = {}): TorrentDetails {
	return { provider_id: 'torr9', external_id: '1', title: 'Dune.2021.MULTi.1080p.WEB.H265-GRP', seeders: 50, ...over };
}

const ingested = { id: 'i1', already_managed: false, snapshot: { infohash: 'abc' } };

function backend(over: Record<string, unknown> = {}) {
	return stubApi({
		'POST /torrents/preview': preview(),
		'GET /search/details?provider=torr9&id=1': details(),
		'GET /me/follows': [],
		'POST /torrents': ingested,
		...over
	});
}

const show = () => render(ReleaseScreen, { provider: 'torr9', id: '1' });

describe('ReleaseScreen', () => {
	beforeEach(() => nav.goto.mockReset());

	it('a page of its own: the title, the release name, the facts, then grab and play', async () => {
		const api = backend();
		await show();
		await expect.element(screen.getByRole('heading', { level: 1, name: 'Dune (2021)' })).toBeVisible();
		await expect.element(screen.getByText('Dune.2021.MULTi.1080p.WEB.H265-GRP', { exact: true })).toBeVisible();
		await expect.element(screen.getByText('50 seeders')).toBeVisible();
		await expect.element(screen.getByText('1 file · 8.0 GB')).toBeVisible();
		await expect.element(screen.getByRole('link', { name: 'Back to results' })).toHaveAttribute('href', '/search');
		await screen.getByRole('button', { name: 'Download and play' }).click();
		await vi.waitFor(() => expect(nav.goto).toHaveBeenCalledWith('/watch/abc/0'));
		expect(api.sent('POST', '/torrents')[0].body).toMatchObject({ provider_id: 'torr9', external_id: '1', allow_duplicate: false });
	});

	it('nobody seeds it: the grab is not operable and says why', async () => {
		const api = backend({ 'GET /search/details?provider=torr9&id=1': details({ seeders: 0 }) });
		await show();
		const play = screen.getByRole('button', { name: 'Download and play' });
		await expect.element(play).toHaveAttribute('aria-disabled', 'true');
		await expect.element(play).toHaveAccessibleDescription(/No seeders right now/);
		await play.click({ force: true });
		expect(api.sent('POST', '/torrents')).toHaveLength(0);
	});

	it('RAR only: refused up front, with the reason', async () => {
		backend({ 'POST /torrents/preview': preview({ streamable: false }) });
		await show();
		const play = screen.getByRole('button', { name: 'Download and play' });
		await expect.element(play).toHaveAttribute('aria-disabled', 'true');
		await expect.element(play).toHaveAccessibleDescription(/RAR archives/);
	});

	it('over 50 GB: the first press asks, only the explicit yes downloads', async () => {
		const api = backend({ 'POST /torrents/preview': preview({ total_size_bytes: 62 * GB }) });
		await show();
		await screen.getByRole('button', { name: 'Download and play' }).click();
		await expect.element(screen.getByText('This release is 62.0 GB. Do you really want all of it?')).toHaveFocus();
		expect(api.sent('POST', '/torrents')).toHaveLength(0);
		await screen.getByRole('button', { name: 'Yes, download 62.0 GB' }).click();
		await vi.waitFor(() => expect(nav.goto).toHaveBeenCalledWith('/watch/abc/0'));
		expect(api.sent('POST', '/torrents')).toHaveLength(1);
	});

	it('already in the library (409): asks before another copy, then sends allow_duplicate', async () => {
		const api = backend({
			'POST /torrents': (c: { body: { allow_duplicate: boolean } }) =>
				c.body.allow_duplicate
					? ingested
					: json({ error: 'duplicate_in_library', message: 'Dune (2021) is already in your library in 1080p.' }, 409)
		});
		await show();
		await screen.getByRole('button', { name: 'Download and play' }).click();
		await expect.element(screen.getByText('Dune (2021) is already in your library in 1080p. Download another copy anyway?')).toBeVisible();
		expect(nav.goto).not.toHaveBeenCalled();
		await screen.getByRole('button', { name: 'Download another copy' }).click();
		await vi.waitFor(() => expect(nav.goto).toHaveBeenCalledWith('/watch/abc/0'));
		expect(api.sent('POST', '/torrents').map((c) => (c.body as { allow_duplicate: boolean }).allow_duplicate)).toEqual([false, true]);
	});

	it('a season pack starts at episode 1; another episode can be chosen', async () => {
		const ep = (i: number, e: number) => ({
			index: i,
			path: `Show.S01/Show.S01E0${e}.1080p.mkv`,
			size_bytes: GB * (3 - e),
			is_video: true,
			is_archive: false,
			extension: 'mkv'
		});
		backend({ 'POST /torrents/preview': preview({ name: 'Show.S01.1080p-GRP', files: [ep(0, 2), ep(1, 1)] }) });
		await show();
		await expect.element(screen.getByRole('heading', { level: 1, name: 'Show · Season 1, complete' })).toBeVisible();
		await expect.element(screen.getByRole('radio', { name: /Show\.S01E01/ })).toBeChecked();
		await screen.getByRole('radio', { name: /Show\.S01E02/ }).click();
		await screen.getByRole('button', { name: 'Download and play episode 2' }).click();
		await vi.waitFor(() => expect(nav.goto).toHaveBeenCalledWith('/watch/abc/0'));
	});

	it('HTML notes: sanitized, the tracker colors and classes dropped, links leave safely', async () => {
		backend({
			'GET /search/details?provider=torr9&id=1': details({
				description_format: 'html',
				description:
					'<p style="color:#ff0000" class="btn" id="main" onclick="alert(1)">Hello <font color="red">red</font><script>alert(1)</script>' +
					'<a href="javascript:alert(1)">bad</a> <a href="https://tracker.test/x">good</a></p>',
				nfo: 'General\nFormat : Matroska'
			})
		});
		await show();
		const notes = screen.getByRole('region', { name: 'Release notes from torr9' });
		await expect.element(notes.getByText(/Hello red/)).toBeVisible();
		const box = notes.getByText(/Hello red/).element().parentElement!;
		expect(box.querySelector('script')).toBeNull();
		expect(box.querySelector('[style], [onclick], [color], .btn, #main')).toBeNull();
		expect(box.querySelector('a[href^="javascript"]')).toBeNull();
		await expect.element(notes.getByRole('link', { name: 'good' })).toHaveAttribute('rel', 'noopener noreferrer');
		await notes.getByRole('button', { name: 'Technical sheet (NFO)' }).click();
		await expect.element(notes.getByText(/Format : Matroska/)).toBeVisible();
		await expect.element(notes.getByText(/Format : Matroska/)).toHaveAttribute('aria-label', 'Technical sheet');
	});

	it('BBCode notes: drawn by us, without the tracker colors; French text says so', async () => {
		backend({
			'GET /search/details?provider=torr9&id=1': details({
				description_format: 'bbcode',
				description:
					'[center][color=#ff0000][b]Dune[/b][/color][/center]\nVoici la description de la release et les sous-titres pour le film.'
			})
		});
		await show();
		const notes = screen.getByRole('region', { name: 'Release notes from torr9' });
		await expect.element(notes.getByText('Dune', { exact: true })).toBeVisible();
		expect(notes.element().querySelector('[style]')).toBeNull();
		expect(notes.element().querySelector('[lang="fr"]')).not.toBeNull();
	});
});
