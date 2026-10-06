import { describe, expect, it } from 'vitest';
import type { AvailableEpisodeEntry, CollectionDetail, CollectionEpisodeEntry, GoneEpisodeEntry } from '@iris/api/client';
import {
	audioChip,
	episodesOf,
	firstSeason,
	mergeEpisodes,
	mergeEpisodesAbsolute,
	nameLanguage,
	qualityWords,
	seasonHolds,
	seasonsOf
} from './merge.ts';

const disk = (season: number, episode: number, language: string, over: Partial<CollectionEpisodeEntry> = {}): CollectionEpisodeEntry => ({
	season,
	episode,
	language,
	infohash: `d-${season}-${episode}-${language}`,
	file_idx: 0,
	watched: false,
	...over
});
const offer = (season: number, episode: number, language: string, over: Partial<AvailableEpisodeEntry> = {}): AvailableEpisodeEntry => ({
	season,
	episode,
	language,
	found_at: '2026-10-01T00:00:00Z',
	indexer_provider: 'c411',
	indexer_torrent_id: `o-${season}-${episode}-${language}`,
	...over
});
const gone = (season: number, episode: number, language: string, over: Partial<GoneEpisodeEntry> = {}): GoneEpisodeEntry => ({
	season,
	episode,
	language,
	infohash: `g-${season}-${episode}-${language}`,
	file_idx: 0,
	watched: true,
	release_name: 'Show.S01E01.FRENCH.1080p',
	source_provider: 'c411',
	source_external_id: '42',
	...over
});

describe('mergeEpisodes', () => {
	it('puts every release of an episode on one row: on disk, then reclaimed, then offers', () => {
		const rows = mergeEpisodes([disk(1, 2, 'french')], [offer(1, 2, 'english'), offer(1, 1, 'english')], [gone(1, 2, 'english')]);
		expect(rows.map((r) => `${r.season}x${r.episode}`)).toEqual(['1x1', '1x2']);
		expect(rows[1].variants.map((v) => `${v.status}:${v.language}`)).toEqual(['downloaded:french', 'gone:english', 'available:english']);
	});

	it('drops a reclaimed release that one on disk in its language (or MULTI) makes pointless', () => {
		expect(mergeEpisodes([disk(1, 1, 'french')], [], [gone(1, 1, 'french')])[0].variants).toHaveLength(1);
		expect(mergeEpisodes([disk(1, 1, 'multi')], [], [gone(1, 1, 'english')])[0].variants).toHaveLength(1);
	});

	it('leaves the season-pack sentinel (episode 0) out of the rows', () => {
		expect(mergeEpisodes([disk(1, 0, 'french')], [offer(2, 0, 'french')])).toEqual([]);
	});
});

describe('mergeEpisodesAbsolute', () => {
	it('is one list on the absolute number; season-cut offers have no place on it', () => {
		const rows = mergeEpisodesAbsolute(
			[disk(1, 1156, 'english', { absolute_episode: 1156 }), disk(23, 2, 'english')],
			[offer(1, 1157, 'english', { absolute_episode: 1157 }), offer(23, 7, 'english')]
		);
		expect(rows.map((r) => r.absolute ?? `S${r.season}E${r.episode}`)).toEqual([1156, 1157, 'S23E2']);
	});
});

describe('seasons', () => {
	it('knows a season from its pack alone, and does not open on the specials', () => {
		const seasons = seasonsOf(mergeEpisodes([disk(0, 1, 'english'), disk(1, 1, 'english')]), [
			{ season: 2, found_at: '', indexer_provider: 'c411', indexer_torrent_id: 'p' }
		]);
		expect(seasons.map((s) => s.season)).toEqual([0, 1, 2]);
		expect(seasons[2].items).toEqual([]);
		expect(firstSeason(seasons)).toBe(1);
	});

	it('never says « 0 episodes » for a season of packs alone', () => {
		const onDisk = [disk(1, 1, 'english'), disk(4, 0, 'english')];
		const seasons = seasonsOf(
			mergeEpisodes(onDisk),
			[{ season: 3, found_at: '', indexer_provider: 'c411', indexer_torrent_id: 'p' }],
			onDisk
		);
		expect(seasons.map((s) => `${s.season}: ${seasonHolds(s)}`)).toEqual(['1: 1 episode', '3: season pack', '4: season pack on disk']);
	});
});

describe('words', () => {
	it('says the audio of a release, the original one marked', () => {
		expect(audioChip('english', 'en')).toBe('English audio (original)');
		expect(audioChip('french', 'en')).toBe('French audio (VF)');
		expect(audioChip('unknown')).toBeNull();
	});

	it('reads a movie copy’s language and picture from its name', () => {
		expect(nameLanguage('Dune.Part.Two.2024.MULTi.1080p.WEB.x265')).toBe('multi');
		expect(nameLanguage('Dune.2021.TRUEFRENCH.720p')).toBe('french');
		expect(nameLanguage('Dune.2021.1080p.BluRay')).toBeNull();
		expect(qualityWords('Severance.S02.1080p.WEB.H265-GRP')).toBe('1080p · HEVC');
		expect(qualityWords('Movie.2160p.UHD.HDR.x265')).toBe('2160p · HEVC · HDR');
	});
});

describe('a title’s rows, merged once', () => {
	const title = (over: Partial<CollectionDetail>) =>
		({
			kind: 'tv',
			numbering: 'seasonal',
			episodes: [disk(1, 2, 'english', { absolute_episode: 14 })],
			available_episodes: [],
			gone_episodes: [],
			...over
		}) as CollectionDetail;

	it('in the series’ own numbering; a movie has none', () => {
		expect(episodesOf(title({}))[0]).toMatchObject({ season: 1, episode: 2, absolute: null });
		expect(episodesOf(title({ numbering: 'absolute' }))[0]).toMatchObject({ absolute: 14 });
		expect(episodesOf(title({ kind: 'movie' }))).toEqual([]);
	});
});
