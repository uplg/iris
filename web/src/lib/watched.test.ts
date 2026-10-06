import { describe, expect, it } from 'vitest';
import type { TitleWatch } from '@iris/api/client';
import { allWatched, isResumable, resumeOf, watchedShare, watchWords } from './watched.ts';

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
	it('the share watched, bounded; unknown without a length; whole once finished', () => {
		expect(watchedShare(50, 100)).toBe(0.5);
		expect(watchedShare(150, 100)).toBe(1);
		expect(watchedShare(5, null)).toBeNull();
		expect(watchedShare(5, 0)).toBeNull();
		expect(watchedShare(0, null, true)).toBe(1);
	});

	it('resumable from 5 s on, the one threshold for resume, the hero and the saves', () => {
		expect(isResumable(4.9)).toBe(false);
		expect(isResumable(5)).toBe(true);
		expect(resumeOf(watch({ position_seconds: 4 }))).toBeNull();
		expect(resumeOf(watch({ position_seconds: 5 }))?.code).toBe('S1:E7');
	});

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
