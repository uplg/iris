import { describe, expect, it } from 'vitest';
import type { TorrentView } from '@iris/api/client';
import { etaSeconds, isComplete, isFetching, isMoving, phaseOf, stateWord } from './torrent.ts';

const t = (over: Partial<TorrentView> = {}) =>
	({
		state: 'live',
		finished: false,
		progress_pct: 40,
		progress_bytes: 400,
		total_size_bytes: 1000,
		peers: 3,
		download_speed_bps: 100,
		...over
	}) as TorrentView;

describe('a release’s state, one rule', () => {
	it('still fetching until all its data is on disk, paused or failed half-way included', () => {
		expect(isFetching(t())).toBe(true);
		expect(isFetching(t({ state: 'paused' }))).toBe(true);
		expect(isFetching(t({ finished: true }))).toBe(false);
		expect(isFetching(t({ progress_pct: 100 }))).toBe(false);
		expect(isFetching(undefined)).toBe(false);
		expect(isComplete(t({ progress_pct: 100 }))).toBe(true);
	});

	it('moving only while it fetches and runs', () => {
		expect(isMoving(t())).toBe(true);
		expect(isMoving(t({ state: 'initializing' }))).toBe(true);
		expect(isMoving(t({ state: 'paused' }))).toBe(false);
		expect(isMoving(t({ state: 'error' }))).toBe(false);
		expect(isMoving(t({ finished: true }))).toBe(false);
	});

	it('one word per state', () => {
		expect(stateWord(t())).toBe('Downloading');
		expect(stateWord(t({ finished: true, progress_pct: 100 }))).toBe('Seeding');
		expect(stateWord(t({ state: 'paused' }))).toBe('Paused');
		expect(stateWord(t({ peers: 0, download_speed_bps: 0 }))).toBe('Stalled');
		expect(stateWord(t({ state: 'error' }))).toBe('Stopped with an error');
		expect(stateWord(t({ state: 'initializing', peers: 0, download_speed_bps: 0 }))).toBe('Checking files');
		// a finished release with nobody to share with still seeds
		expect(phaseOf(t({ finished: true, peers: 0, download_speed_bps: 0 }))).toBe('seeding');
	});

	it('the time left at the current speed, none when nothing moves', () => {
		expect(etaSeconds(t())).toBe(6);
		expect(etaSeconds(t({ download_speed_bps: 0 }))).toBeNull();
	});
});
