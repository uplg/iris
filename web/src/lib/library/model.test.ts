import { describe, expect, it } from 'vitest';
import { collection, torrent } from './fixtures.ts';
import {
	activityByCollection,
	deleteDescription,
	filterTitles,
	groupOf,
	releaseName,
	releaseStatus,
	seasonOf,
	titleCounts,
	titleStatus,
	type TitleFilters
} from './model.ts';

const MB = 1024 ** 2;
const downloading = torrent({
	infohash: 'dl',
	name: 'Severance.S04.1080p.WEB.x264-GRP',
	finished: false,
	progress_pct: 42,
	progress_bytes: 420 * MB,
	total_size_bytes: 1000 * MB,
	download_speed_bps: 6.1 * MB,
	peers: 24
});

describe('releases', () => {
	it('a release name drops a single file’s extension', () => {
		expect(releaseName('Midnight.2021.1080p.WEB.x264-FW.mkv')).toBe('Midnight.2021.1080p.WEB.x264-FW');
		expect(releaseName('Show.S01.1080p')).toBe('Show.S01.1080p');
	});

	it('go to their group: downloading, needing a hand, seeding', () => {
		expect(groupOf(downloading)).toBe('downloading');
		expect(groupOf(torrent())).toBe('seeding');
		expect(groupOf(torrent({ finished: false, progress_pct: 10 }))).toBe('attention');
		expect(groupOf(torrent({ state: 'error' }))).toBe('attention');
		expect(groupOf(torrent({ state: 'paused' }))).toBe('attention');
		expect(groupOf(torrent({ state: 'initializing', finished: false, progress_pct: 3 }))).toBe('downloading');
	});

	it('say their state in words', () => {
		expect(releaseStatus(downloading).text).toBe('Downloading · 42% · 6.1 MB/s · 24 peers · done in about 2 min');
		expect(releaseStatus(torrent({ state: 'paused', source_provider: 'nyaa' })).text).toBe(
			'Paused after download · nyaa releases never seed'
		);
		expect(releaseStatus(torrent({ finished: false, progress_pct: 10 })).text).toBe('Stalled · no peers · 10%');
		expect(releaseStatus(torrent({ state: 'error', error: 'disk full' })).text).toBe('Stopped with an error · disk full');
		expect(releaseStatus(torrent({ peers: 3, upload_speed_bps: MB })).text).toBe('Seeding · 3 peers downloading · 1.0 MB/s up');
		expect(releaseStatus(torrent()).text).toBe('Seeding · nobody downloading now · 0 B/s up');
	});

	it('a delete names the files it removes', () => {
		const files = ['a', 'b', 'c', 'd', 'e'].map((n, index) => ({ index, path: `dir/${n}.mkv`, size_bytes: 1 }));
		expect(deleteDescription(torrent({ files }))).toContain('5 files (a.mkv, b.mkv, c.mkv and 2 more)');
	});
});

describe('titles', () => {
	const movie = collection({ id: 'm', display_title: 'Arrival', kind: 'movie', episode_count: 0, torrent_count: 1, total_size_bytes: 5 });
	const anime = collection({ id: 'a', display_title: 'Frieren', is_anime: true, total_size_bytes: 50 * 1024 ** 3 });
	const ghost = collection({ id: 'g', display_title: 'Dark', ghost: true });
	const series = collection();
	const all = [series, movie, anime, ghost];
	const f = (over: Partial<TitleFilters> = {}): TitleFilters => ({ query: '', type: 'all', show: 'all', sort: 'recent', ...over });

	it('are counted by type, ghosts left out', () => {
		expect(titleCounts(all)).toBe('3 titles · 1 movie · 1 series · 1 anime');
	});

	it('say their state in words', () => {
		expect(titleStatus(series, undefined).text).toBe('19 episodes on disk');
		expect(titleStatus(movie, undefined).text).toBe('On disk');
		expect(titleStatus(ghost, undefined).text).toBe('No longer on disk');
		const activity = activityByCollection([downloading]);
		expect(titleStatus(series, activity.get('c-1'))).toEqual({ tone: 'busy', text: 'Downloading Season 4 · 42%' });
	});

	it('the season or episode a release carries', () => {
		expect(seasonOf('Show.S04.1080p')).toBe('Season 4');
		expect(seasonOf('Show_S02E07_1080p')).toBe('S2:E7');
		expect(seasonOf('Show.S02E07.1080p')).toBe('S2:E7');
		expect(seasonOf('Movie.2021.1080p')).toBeNull();
	});

	it('filter by type, state and name, and sort', () => {
		const act = activityByCollection([downloading]);
		expect(filterTitles(all, f({ type: 'movie' }), act).map((c) => c.id)).toEqual(['m']);
		expect(filterTitles(all, f({ type: 'anime' }), act).map((c) => c.id)).toEqual(['a']);
		expect(filterTitles(all, f({ type: 'series' }), act).map((c) => c.id)).toEqual(['c-1', 'g']);
		expect(filterTitles(all, f({ show: 'gone' }), act).map((c) => c.id)).toEqual(['g']);
		expect(filterTitles(all, f({ show: 'downloading' }), act).map((c) => c.id)).toEqual(['c-1']);
		expect(filterTitles(all, f({ query: 'fri' }), act).map((c) => c.id)).toEqual(['a']);
		expect(filterTitles(all, f({ sort: 'title' }), act).map((c) => c.id)).toEqual(['m', 'g', 'a', 'c-1']);
		expect(filterTitles(all, f({ sort: 'size' }), act)[0].id).toBe('a');
	});

	it('recently watched comes first in that sort', () => {
		const act = activityByCollection([torrent({ collection_id: 'm', last_played_at: '2026-10-05T10:00:00Z' })]);
		expect(filterTitles(all, f({ sort: 'watched' }), act)[0].id).toBe('m');
	});
});
