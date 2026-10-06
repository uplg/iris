import { afterEach, describe, expect, it } from 'vitest';
import type { TorrentView } from '@iris/api/client';
import { refetchCollection } from '#lib/collection/actions.ts';
import { queryClient } from '#lib/query.ts';
import { FAST, KEYS, playbackPrefsSaved, read, refreshLibrary, SLOW } from './queries.ts';

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

describe('a release’s positions', () => {
	afterEach(() => queryClient.clear());

	it('are read once per release, not again on each return to the tab', () => {
		expect(read.progress('ih')).toMatchObject({ queryKey: KEYS.progress('ih'), refetchOnWindowFocus: false });
	});

	it('are read again after a gesture on the title', async () => {
		queryClient.setQueryData(KEYS.progress('ih'), []);
		await refetchCollection('c1');
		expect(invalidated(KEYS.progress('ih'))).toBe(true);
	});
});

describe('a release deleted, paused or grabbed', () => {
	afterEach(() => queryClient.clear());

	it('reads its title page and its own page again, not only the library', async () => {
		queryClient.setQueryData(KEYS.collection('c1'), {});
		queryClient.setQueryData(KEYS.torrent('ih'), {});
		queryClient.setQueryData(KEYS.torrents, {});
		await refreshLibrary();
		expect(invalidated(KEYS.collection('c1'))).toBe(true);
		expect(invalidated(KEYS.torrent('ih'))).toBe(true);
		expect(invalidated(KEYS.torrents)).toBe(true);
	});
});
