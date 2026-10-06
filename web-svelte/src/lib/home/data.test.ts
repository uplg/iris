import { describe, expect, it } from 'vitest';
import type { TorrentView } from '@iris/api/client';
import { downloadsByCollection, languagesLine, rightNow, secondsLeft, watched } from './data.ts';

describe('home words', () => {
	it('"Right now": only what is happening, in words', () => {
		expect(
			rightNow({
				downloading: 2,
				downloading_pct: 63.6,
				downloading_eta_seconds: 1380,
				new_episodes: 3,
				seeding: 18,
				disk: { free_bytes: 412 * 1024 ** 3, total_bytes: 2000 * 1024 ** 3 }
			})
		).toEqual(['2 downloads · 64% · 23 min left', '3 new episodes on your watchlist', '412 GB free on disk', 'Seeding 18 releases']);
		expect(rightNow({ downloading: 1, downloading_pct: 10, new_episodes: 1, seeding: 1 })).toEqual([
			'1 download · 10%',
			'1 new episode on your watchlist',
			'Seeding 1 release'
		]);
		expect(rightNow({ downloading: 0, downloading_pct: 0, new_episodes: 0, seeding: 0 })).toEqual([]);
	});

	it('downloads per collection, by bytes; finished ones and loose torrents left out', () => {
		const t = (collection_id: string | null, done: number, total: number, finished = false) =>
			({ collection_id, progress_bytes: done, total_size_bytes: total, finished }) as TorrentView;
		const m = downloadsByCollection([t('a', 1, 4), t('a', 3, 4), t('b', 5, 5, true), t(null, 1, 2)]);
		expect([...m]).toEqual([['a', 50]]);
	});

	it('time left and share watched only when the length is known', () => {
		expect(secondsLeft({ position_seconds: 1930, duration_seconds: 3300 })).toBe(1370);
		expect(watched({ position_seconds: 1650, duration_seconds: 3300 })).toBe(0.5);
		expect(secondsLeft({ position_seconds: 10, duration_seconds: null })).toBeNull();
	});

	it('says the languages a play will use, or nothing', () => {
		expect(languagesLine({ audio_language: 'fr', subtitle_language: 'off' })).toBe('Plays with audio in French, subtitles off.');
		expect(languagesLine({ subtitle_language: 'en', for_collection: true })).toBe(
			'Plays with subtitles in English, as chosen for this series.'
		);
		expect(languagesLine({})).toBeNull();
	});
});
