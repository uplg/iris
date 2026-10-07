import { describe, expect, it } from 'vitest';
import type { AvailableEpisodeEntry, CollectionEpisodeEntry } from '@iris/api/client';
import { currentLanguage, sideRows, type SideInput } from './episodes.ts';

const ep = (
	season: number,
	episode: number,
	infohash: string,
	file_idx: number,
	language: string | null,
	watched = false
): CollectionEpisodeEntry => ({
	season,
	episode,
	infohash,
	file_idx,
	language,
	watched
});
const av = (season: number, episode: number, language: string | null): AvailableEpisodeEntry => ({
	season,
	episode,
	language,
	found_at: '',
	indexer_provider: 'x',
	indexer_torrent_id: '1'
});
const base = (o: Partial<SideInput>): SideInput => ({
	infohash: 'h1',
	fileIdx: 0,
	isTvCollection: true,
	episodes: [],
	available: [],
	videoFiles: [],
	progressByFile: new Map(),
	...o
});

describe('the side panel', () => {
	it('lists the current season, one row per episode, in order', () => {
		const rows = sideRows(
			base({
				episodes: [ep(2, 2, 'h2', 0, 'fr'), ep(2, 1, 'h1', 0, 'fr'), ep(1, 1, 'h0', 0, 'fr'), ep(2, 2, 'h3', 0, 'en')],
				available: [av(2, 3, 'en'), av(2, 3, 'fr'), av(2, 2, 'en')]
			})
		);
		expect(rows.map((r) => [r.primary, r.secondary, r.infohash, !!r.grab])).toEqual([
			['S2:E1', 'fr', 'h1', false],
			['S2:E2', 'fr', 'h2', false],
			['S2:E3', 'fr', '', true]
		]);
		expect(rows[0].active).toBe(true);
	});

	it('keeps the playing file’s row whatever its language', () => {
		const rows = sideRows(base({ infohash: 'h3', episodes: [ep(2, 2, 'h2', 0, 'fr'), ep(2, 2, 'h3', 0, 'en'), ep(2, 1, 'hx', 0, 'fr')] }));
		expect(rows.find((r) => r.primary === 'S2:E2')?.infohash).toBe('h3');
	});

	it('lists every season when the playing file is not indexed yet', () => {
		const rows = sideRows(base({ infohash: 'zz', episodes: [ep(1, 1, 'h0', 0, null), ep(2, 1, 'h1', 1, null)] }));
		expect(rows.map((r) => r.primary)).toEqual(['S1:E1', 'S2:E1']);
	});

	it('lists the torrent’s files otherwise, with where one stopped', () => {
		const rows = sideRows(
			base({
				isTvCollection: false,
				fileIdx: 1,
				videoFiles: [
					{ index: 0, path: 'Pack/E01.mkv', size_bytes: 1024 },
					{ index: 1, path: 'Pack/E02.mkv', size_bytes: 2048 }
				],
				progressByFile: new Map([[0, { file_idx: 0, position_seconds: 50, duration_seconds: 100, completed: false, last_watched_at: '' }]])
			})
		);
		expect(rows.map((r) => [r.primary, r.secondary, r.watchedPct, r.active])).toEqual([
			['E01.mkv', '1.0 KB', 50, false],
			['E02.mkv', '2.0 KB', null, true]
		]);
	});

	it('prefers the playing language, else the series’ dominant one', () => {
		expect(currentLanguage(ep(1, 1, 'h', 0, 'en'), [])).toBe('en');
		expect(currentLanguage(undefined, [ep(1, 1, 'a', 0, 'fr'), ep(1, 2, 'b', 0, 'fr'), ep(1, 3, 'c', 0, 'en')])).toBe('fr');
		expect(currentLanguage(ep(1, 1, 'h', 0, 'unknown'), [])).toBeNull();
	});
});
