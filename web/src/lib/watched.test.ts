import { describe, expect, it } from 'vitest';
import type { TitleWatch } from '@iris/api/client';
import { allWatched, resumeOf, watchWords } from './watched.ts';

const watch = (w: Partial<TitleWatch> = {}): TitleWatch => ({
	infohash: 'aa',
	file_idx: 3,
	season: 1,
	episode: 7,
	position_seconds: 600,
	duration_seconds: 2400,
	completed: false,
	last_watched_at: '2026-10-06T10:00:00Z',
	watched_episodes: 6,
	...w
});

describe('watched', () => {
	it('resumes a file left mid-way, with what is left', () => {
		expect(resumeOf(watch())).toEqual({ infohash: 'aa', fileIdx: 3, code: 'S1:E7', left: 1800, share: 0.25 });
		expect(resumeOf(watch({ position_seconds: 2 }))).toBeNull();
		expect(resumeOf(watch({ completed: true }))).toBeNull();
		expect(resumeOf(null)).toBeNull();
	});

	it('says a series is watched once every episode on disk is', () => {
		expect(allWatched(watch({ watched_episodes: 10, completed: true }), 'tv', 10)).toBe(true);
		expect(allWatched(watch(), 'tv', 10)).toBe(false);
		expect(allWatched(watch({ completed: true }), 'movie', 1)).toBe(true);
	});

	it('puts it in words', () => {
		expect(watchWords(watch(), 'tv', 10)).toBe('In progress · S1:E7 · 30 min left');
		expect(watchWords(watch({ completed: true }), 'tv', 10)).toBe('Last watched S1:E7');
		expect(watchWords(watch({ completed: true, watched_episodes: 10 }), 'tv', 10)).toBe('Watched');
		expect(watchWords(undefined, 'tv', 10)).toBeNull();
	});
});
