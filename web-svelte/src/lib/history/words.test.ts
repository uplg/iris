import { describe, expect, it } from 'vitest';
import { plural } from '@iris/api/format';
import { fileName, onDay, progressWords, since, watchedShare, whatWatched } from './words.ts';

const now = new Date(2026, 9, 6, 15, 0).getTime();

describe('onDay', () => {
	it('today, yesterday and tomorrow with the time; this week by its day; else the date', () => {
		expect(onDay(new Date(2026, 9, 6, 9, 5).getTime(), now)).toBe('today at 09:05');
		expect(onDay(new Date(2026, 9, 5, 23, 59).getTime(), now)).toBe('yesterday at 23:59');
		expect(onDay(new Date(2026, 9, 7, 8, 0).getTime(), now)).toBe('tomorrow at 08:00');
		expect(onDay(new Date(2026, 9, 2, 12, 0).getTime(), now)).toBe('on Friday');
		expect(onDay(new Date(2026, 8, 20, 12, 0).getTime(), now)).toBe('on 20 Sept');
		expect(onDay(new Date(2025, 8, 20, 12, 0).getTime(), now)).toBe('on 20 Sept 2025');
	});
});

describe('progress in words', () => {
	it('finished, started, or how far and where it stopped', () => {
		expect(progressWords(10, 100, true)).toBe('Watched to the end');
		expect(progressWords(0, 3000)).toBe('Not started');
		expect(progressWords(1930, 3300)).toBe('58% watched, stopped at 32:10');
		expect(progressWords(65, null)).toBe('Stopped at 1:05');
	});

	it('the share for a bar, bounded', () => {
		expect(watchedShare(50, 100)).toBe(0.5);
		expect(watchedShare(150, 100)).toBe(1);
		expect(watchedShare(5, null)).toBe(0);
		expect(watchedShare(0, 100, true)).toBe(1);
	});

	it('how long a session has lasted', () => {
		expect(since(new Date(now - 20_000).toISOString(), now)).toBe('for under a minute');
		expect(since(new Date(now - 12 * 60_000).toISOString(), now)).toBe('for 12 min');
		expect(since(new Date(now - 65 * 60_000).toISOString(), now)).toBe('for 1 h 5 min');
	});
});

describe('what was watched', () => {
	it('an absolute episode, an episode code, a season, else the file', () => {
		expect(whatWatched({ absolute_episode: 1156, season: 21, episode: 3 })).toBe('Episode 1156');
		expect(whatWatched({ season: 2, episode: 4 })).toBe('S2:E4');
		expect(whatWatched({ season: 2, episode: 0 })).toBe('Season 2');
		expect(whatWatched({ file_path: 'Show/S01/Film.2024.mkv' })).toBe('Film.2024.mkv');
		expect(whatWatched({})).toBeNull();
		expect(fileName(null)).toBeNull();
	});

	it('counts with the right word', () => {
		expect(plural(1, 'title')).toBe('1 title');
		expect(plural(3, 'title')).toBe('3 titles');
		expect(plural(2, 'entry', 'entries')).toBe('2 entries');
	});
});
