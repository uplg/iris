import { afterEach, describe, expect, it } from 'vitest';
import type { TorrentView } from '@iris/api/client';
import { queryClient } from '#lib/query.ts';
import { FAST, KEYS, playbackPrefsSaved, read, SLOW } from './queries.ts';

const prefs = { audio_language: 'fr', subtitle_language: null };
const invalidated = (key: readonly unknown[]) => queryClient.getQueryState(key)?.isInvalidated;

describe('playback languages saved', () => {
	afterEach(() => queryClient.clear());

	it('a series’ own choice reads that series again only', async () => {
		queryClient.setQueryData(KEYS.playbackPrefs('c1'), prefs);
		queryClient.setQueryData(KEYS.playbackPrefs('c2'), prefs);
		await playbackPrefsSaved('c1');
		expect(invalidated(KEYS.playbackPrefs('c1'))).toBe(true);
		expect(invalidated(KEYS.playbackPrefs('c2'))).toBe(false);
	});

	it('the account-wide one is every series’ fallback: all are read again', async () => {
		queryClient.setQueryData(KEYS.playbackPrefs(null), prefs);
		queryClient.setQueryData(KEYS.playbackPrefs('c1'), prefs);
		await playbackPrefsSaved(null);
		expect(invalidated(KEYS.playbackPrefs(null))).toBe(true);
		expect(invalidated(KEYS.playbackPrefs('c1'))).toBe(true);
	});
});

describe('one release, live', () => {
	const interval = (data?: Partial<TorrentView>) =>
		read.torrent('ih').refetchInterval({ state: { data: data as TorrentView | undefined } });

	it('polls quickly while it fetches data, slowly once it only shares', () => {
		expect(interval()).toBe(FAST);
		expect(interval({ finished: false, progress_pct: 40, state: 'live' })).toBe(FAST);
		expect(interval({ finished: true, progress_pct: 100, state: 'live' })).toBe(SLOW);
		expect(interval({ finished: false, progress_pct: 40, state: 'paused' })).toBe(SLOW);
	});
});
