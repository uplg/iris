import { describe, expect, it } from 'vitest';
import { fileName, onDay, plural, since } from '@iris/api/format';
import { playName, progressShort, progressWords, watchedShare, whatWatched } from './words.ts';

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

describe('a play, named as people name it', () => {
	it('the title and its episode, never the release name when the title is known', () => {
		const ep = { collection_title: 'Severance', torrent_name: 'Severance.S02.1080p.WEB-GRP', kind: 'tv', season: 2, episode: 4 };
		expect(playName({ ...ep, episode_title: 'Woe’s Hollow' })).toEqual({ title: 'Severance', detail: 'S2:E4 · Woe’s Hollow' });
		expect(playName(ep)).toEqual({ title: 'Severance', detail: 'S2:E4' });
		expect(playName({ collection_title: 'One Piece', kind: 'tv', absolute_episode: 1156, season: 21, episode: 3 }).detail).toBe(
			'Episode 1156'
		);
		expect(playName({ collection_title: 'Dune', kind: 'movie', year: 2021 })).toEqual({ title: 'Dune', detail: 'Film · 2021' });
		expect(playName({ collection_title: 'Dune', kind: 'movie' }).detail).toBe('Film');
	});

	it('no title: the release name cleaned up; a pack file still names its episode', () => {
		expect(playName({ torrent_name: 'Mercato.2025.FRENCH.1080p.WEB.H265-BOUBA' }).title).toBe('Mercato (2025)');
		expect(playName({ torrent_name: 'Show.S01.1080p', file_path: 'Show.S01/Show.S01E03.1080p.mkv' })).toEqual({
			title: 'Show',
			detail: 'S1:E3'
		});
		expect(playName({}).title).toBe('Something unnamed');
	});

	it('how far, short', () => {
		expect(progressShort(10, 100, true)).toBe('Finished');
		expect(progressShort(20, 3000)).toBe('Just started');
		expect(progressShort(42 * 60, 130 * 60)).toBe('42 min of 2 h 10 min');
		expect(progressShort(1930, null)).toBe('Stopped at 32:10');
	});
});
