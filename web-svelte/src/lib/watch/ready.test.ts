import { describe, expect, it } from 'vitest';
import type { TorrentView } from '@iris/api/client';
import { isDeadSwarm, readiness, type ReadyInput } from './ready.ts';

const torrent = (o: Partial<TorrentView> = {}): TorrentView =>
	({
		state: 'live',
		finished: false,
		progress_pct: 12,
		peers: 4,
		download_speed_bps: 6_400_000,
		added_at: '2026-10-06T10:00:00Z',
		fetched_at: '2026-10-06T10:01:00Z',
		files: [],
		...o
	}) as TorrentView;

const input = (o: Partial<ReadyInput> = {}): ReadyInput => ({
	torrent: torrent(),
	serverPrep: false,
	hasProbe: false,
	hasManifest: false,
	probeFetching: false,
	probeError: null,
	progressPending: false,
	playStatus: null,
	playError: null,
	...o
});
const states = (r: ReturnType<typeof readiness>) => Object.fromEntries(r.steps.map((s) => [s.id, s.state]));

describe('getting ready', () => {
	it('connects to peers first', () => {
		const r = readiness(input({ torrent: torrent({ state: 'initializing' }) }));
		expect(r.steps[0]).toMatchObject({ id: 'peers', state: 'current' });
	});

	it('downloads the head with its percentage and throughput', () => {
		const r = readiness(input({ probeError: new Error('file not yet on disk') }));
		expect(r.problem).toBeNull();
		expect(r.steps.find((s) => s.id === 'head')).toMatchObject({
			state: 'current',
			label: 'Downloading the first minutes, 12%',
			detail: '6.1 MB/s from 4 peers',
			pct: 12
		});
	});

	it('then reads the file; a direct tier starts once the manifest is in', () => {
		expect(states(readiness(input({ probeFetching: true })))).toMatchObject({
			peers: 'done',
			head: 'done',
			read: 'current',
			start: 'waiting'
		});
		expect(states(readiness(input({ hasManifest: true })))).toMatchObject({ read: 'done', resume: 'done', start: 'current' });
		expect(readiness(input()).steps.some((s) => s.id === 'server')).toBe(false);
	});

	it('Tier F waits for the server: downloading, then remuxing, in words with its percentage', () => {
		const downloading = readiness(
			input({ serverPrep: true, hasProbe: true, playStatus: { ready: false, reason: 'downloading', progress: 0.3 } })
		);
		expect(downloading.steps.find((s) => s.id === 'server')).toMatchObject({
			state: 'current',
			label: 'Downloading on the server, 30%',
			pct: 30
		});
		const remuxing = readiness(
			input({ serverPrep: true, hasProbe: true, playStatus: { ready: false, reason: 'remuxing', progress: 0.995 } })
		);
		expect(remuxing.steps.find((s) => s.id === 'server')).toMatchObject({ label: 'Remuxing on the server, 99%', pct: 99 });
		const starting = readiness(input({ serverPrep: true, hasProbe: true, playStatus: { ready: false, reason: 'remuxing' } }));
		expect(starting.steps.find((s) => s.id === 'server')?.label).toBe('Remuxing on the server');
		const ready = readiness(input({ serverPrep: true, hasProbe: true, playStatus: { ready: true } }));
		expect(states(ready)).toMatchObject({ server: 'done', start: 'current' });
	});

	it('says a problem plainly', () => {
		expect(readiness(input({ torrent: torrent({ state: 'error', error: 'disk full' }) })).problem).toMatchObject({
			title: 'The torrent stopped with an error',
			detail: 'disk full'
		});
		expect(readiness(input({ probeError: new Error('ffprobe crashed') })).problem?.title).toBe('Iris could not read this file');
		expect(readiness(input({ serverPrep: true, playStatus: { ready: false, error: 'remux failed' } })).problem?.detail).toBe(
			'remux failed'
		);
		// a remux failure is no problem for a tier that does not wait for it
		expect(readiness(input({ playStatus: { ready: false, error: 'remux failed' } })).problem).toBeNull();
	});

	it('knows a dead swarm, from the backend or from the snapshot', () => {
		expect(isDeadSwarm(torrent(), new Error('no seeders answered'))).toBe(true);
		expect(isDeadSwarm(torrent(), new Error('stalled: nothing for 30 s'))).toBe(true);
		const dead = torrent({ peers: 0, download_speed_bps: 0, fetched_at: '2026-10-06T10:03:00Z' });
		expect(isDeadSwarm(dead, null)).toBe(true);
		// a fresh add still looking for its swarm is not dead yet
		expect(isDeadSwarm(torrent({ peers: 0, download_speed_bps: 0 }), null)).toBe(false);
		expect(isDeadSwarm({ ...dead, finished: true }, null)).toBe(false);
		const r = readiness(input({ torrent: dead }));
		expect(r.problem).toMatchObject({ title: 'Nobody is sharing this release', deadSwarm: true });
	});
});
