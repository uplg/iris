import { describe, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import type { PlayStatus, TorrentView } from '@iris/api/client';
import GettingReady from './GettingReady.svelte';
import { readiness } from './ready.ts';

const torrent = (o: Partial<TorrentView> = {}) =>
	({
		state: 'live',
		finished: false,
		progress_pct: 30,
		peers: 3,
		download_speed_bps: 2_000_000,
		added_at: '2026-10-06T10:00:00Z',
		fetched_at: '2026-10-06T10:01:00Z',
		files: [],
		...o
	}) as TorrentView;

/** The steps for a Tier F file whose play status answers `status`. */
const forStatus = (status: PlayStatus | null, t = torrent()) =>
	readiness({
		torrent: t,
		serverPrep: true,
		hasProbe: true,
		hasManifest: true,
		probeFetching: false,
		probeError: null,
		progressPending: false,
		playStatus: status,
		playError: null
	});

describe('GettingReady', () => {
	it('says each step in words, the current one with how far it is', async () => {
		await render(GettingReady, { ready: forStatus({ ready: false, reason: 'remuxing', progress: 0.42 }) });
		await expect.element(page.getByRole('heading', { name: 'Getting ready' })).toBeVisible();
		await expect.element(page.getByText('Connecting to peers, Done')).toBeInTheDocument();
		await expect.element(page.getByText('Remuxing on the server, 42%, In progress')).toBeInTheDocument();
		await expect.element(page.getByText('Starting playback, Waiting')).toBeInTheDocument();
		await expect.element(page.getByRole('progressbar', { name: 'Remuxing on the server, 42%' })).toHaveAttribute('aria-valuetext', '42%');
		await expect.element(page.getByRole('status')).toHaveTextContent('Preparing the stream on the server');
	});

	it('follows the play status from download to remux', async () => {
		const screen = await render(GettingReady, { ready: forStatus({ ready: false, reason: 'downloading', progress: 0.1 }) });
		await expect.element(page.getByText('Downloading on the server, 10%, In progress')).toBeInTheDocument();
		await expect.element(page.getByText('1.9 MB/s from 3 peers')).toBeVisible();
		await screen.rerender({ ready: forStatus({ ready: false, reason: 'remuxing' }) });
		await expect.element(page.getByText('Remuxing on the server, In progress')).toBeInTheDocument();
		await screen.rerender({ ready: forStatus({ ready: true }) });
		await expect.element(page.getByText('Starting playback, In progress')).toBeInTheDocument();
	});

	it('says a failed preparation as a problem', async () => {
		await render(GettingReady, { ready: forStatus({ ready: false, error: 'ffmpeg exited' }) });
		await expect.element(page.getByRole('alert').getByRole('heading', { name: 'The server could not prepare this file' })).toBeVisible();
		await expect.element(page.getByText('ffmpeg exited')).toBeVisible();
	});

	it('offers another release when nobody shares this one', async () => {
		const onreplace = vi.fn();
		const dead = torrent({ peers: 0, download_speed_bps: 0, fetched_at: '2026-10-06T10:05:00Z' });
		await render(GettingReady, { ready: forStatus(null, dead), onreplace });
		await expect.element(page.getByRole('alert').getByRole('heading', { name: 'Nobody is sharing this release' })).toBeVisible();
		await page.getByRole('button', { name: 'Remove it and pick another release' }).click();
		expect(onreplace).toHaveBeenCalledOnce();
	});
});
