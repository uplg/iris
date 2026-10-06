// A collection's episodes as one row per episode, whatever holds it: releases on disk, offers
// the indexer cached, releases the GC reclaimed (web/src/pages/CollectionPage.tsx). Plus the
// words the page says about them.

import type {
	AvailableEpisodeEntry,
	CollectionDetail,
	CollectionEpisodeEntry,
	ContinueWatchingItem,
	GoneEpisodeEntry,
	SeasonPackEntry,
	TorrentView
} from '@iris/api/client';
import { episodeCode, isVideo, languageLabel } from '@iris/api/format';

export type Downloaded = {
	status: 'downloaded';
	language: string | null;
	infohash: string;
	file_idx: number;
	watched: boolean;
	/** The person's place in it and its length, as the server last saved them (null: not started). */
	position_seconds: number | null;
	duration_seconds: number | null;
};
export type Available = {
	status: 'available';
	language: string | null;
	indexer_provider: string;
	indexer_torrent_id: string;
	quality: string | null;
	seeders: number | null;
	size_bytes: number | null;
};
/** Reclaimed: shown in place with « Download again » (same infohash: the saved position resumes). */
export type Gone = {
	status: 'gone';
	language: string | null;
	infohash: string;
	file_idx: number;
	watched: boolean;
	release_name: string;
	quality: string | null;
	total_size_bytes: number;
	source_provider: string;
	source_external_id: string;
};
export type Variant = Downloaded | Available | Gone;

export type Episode = {
	season: number;
	episode: number;
	/** Fleuve anime only: the row reads « Episode N »; (season, episode) stay for the grab call. */
	absolute: number | null;
	variants: Variant[];
};

/** A release on disk in the same language (or MULTI) makes « Download again » noise. */
function pruneShadowedGone(variants: Variant[]): Variant[] {
	const owned = new Set(variants.filter((v) => v.status === 'downloaded').map((v) => v.language ?? ''));
	if (owned.size === 0) return variants;
	const multi = owned.has('multi');
	return variants.filter((v) => v.status !== 'gone' || (!multi && !owned.has(v.language ?? '')));
}

const RANK = { downloaded: 0, gone: 1, available: 2 } as const;

function settle(rows: Iterable<Episode>) {
	for (const row of rows) {
		row.variants = pruneShadowedGone(row.variants);
		row.variants = row.variants.toSorted((a, b) => RANK[a.status] - RANK[b.status] || (a.language ?? '').localeCompare(b.language ?? ''));
	}
}

const downloaded = (d: CollectionEpisodeEntry): Downloaded => ({
	status: 'downloaded',
	language: d.language ?? null,
	infohash: d.infohash,
	file_idx: d.file_idx,
	watched: d.watched,
	position_seconds: d.position_seconds ?? null,
	duration_seconds: d.duration_seconds ?? null
});

const gone = (g: GoneEpisodeEntry): Gone => ({
	status: 'gone',
	language: g.language ?? null,
	infohash: g.infohash,
	file_idx: g.file_idx,
	watched: g.watched,
	release_name: g.release_name,
	quality: g.quality ?? null,
	total_size_bytes: g.total_size_bytes ?? 0,
	source_provider: g.source_provider,
	source_external_id: g.source_external_id
});

const available = (a: AvailableEpisodeEntry): Available => ({
	status: 'available',
	language: a.language ?? null,
	indexer_provider: a.indexer_provider,
	indexer_torrent_id: a.indexer_torrent_id,
	quality: a.quality ?? null,
	seeders: a.seeders ?? null,
	size_bytes: a.size_bytes ?? null
});

/** Seasonal layout: one row per (season, episode). Episode 0 is the season-pack sentinel. */
export function mergeEpisodes(
	onDisk: CollectionEpisodeEntry[],
	offers: AvailableEpisodeEntry[] = [],
	reclaimed: GoneEpisodeEntry[] = []
): Episode[] {
	const rows = new Map<string, Episode>();
	const ensure = (season: number, episode: number) => {
		const key = `${season}-${episode}`;
		let row = rows.get(key);
		if (!row) rows.set(key, (row = { season, episode, absolute: null, variants: [] }));
		return row;
	};
	for (const d of onDisk) if (d.episode !== 0) ensure(d.season, d.episode).variants.push(downloaded(d));
	for (const g of reclaimed) if (g.episode !== 0) ensure(g.season, g.episode).variants.push(gone(g));
	for (const a of offers) if (a.episode !== 0) ensure(a.season, a.episode).variants.push(available(a));
	settle(rows.values());
	return [...rows.values()].toSorted((a, b) => a.season - b.season || a.episode - b.episode);
}

/**
 * Absolute layout (One Piece): one flat list on the absolute number. Season-cut offers
 * (`S23E07`, no absolute) have no place on that axis and are left out, or they would alias onto
 * low rows; what is on disk (or was) always shows, by its SxxExx when it has no absolute.
 */
export function mergeEpisodesAbsolute(
	onDisk: CollectionEpisodeEntry[],
	offers: AvailableEpisodeEntry[] = [],
	reclaimed: GoneEpisodeEntry[] = []
): Episode[] {
	const rows = new Map<string, Episode>();
	const ensure = (abs: number | null, season: number, episode: number) => {
		const key = abs !== null ? `a:${abs}` : `s:${season}:${episode}`;
		let row = rows.get(key);
		if (!row) rows.set(key, (row = { season, episode, absolute: abs, variants: [] }));
		return row;
	};
	for (const d of onDisk) if (d.episode !== 0) ensure(d.absolute_episode ?? null, d.season, d.episode).variants.push(downloaded(d));
	for (const g of reclaimed) if (g.episode !== 0) ensure(g.absolute_episode ?? null, g.season, g.episode).variants.push(gone(g));
	for (const a of offers) {
		if (a.episode === 0 || typeof a.absolute_episode !== 'number') continue;
		ensure(a.absolute_episode, a.season, a.episode).variants.push(available(a));
	}
	settle(rows.values());
	return [...rows.values()].toSorted((a, b) => {
		if (a.absolute !== null && b.absolute !== null) return a.absolute - b.absolute;
		if (a.absolute !== null) return -1;
		if (b.absolute !== null) return 1;
		return a.season - b.season || a.episode - b.episode;
	});
}

/** A series' rows, in its own numbering (merged once per read: the page and its head share them). */
export function episodesOf(c: CollectionDetail): Episode[] {
	if (c.kind !== 'tv') return [];
	const merge = c.numbering === 'absolute' ? mergeEpisodesAbsolute : mergeEpisodes;
	return merge(c.episodes, c.available_episodes, c.gone_episodes);
}

export type Season = { season: number; items: Episode[]; packs: SeasonPackEntry[] };

/** The seasons known, from episodes and from pack-only seasons (a pack the only signal yet). */
export function seasonsOf(episodes: Episode[], packs: SeasonPackEntry[] = []): Season[] {
	const by = new Map<number, Season>();
	const ensure = (s: number) => {
		let row = by.get(s);
		if (!row) by.set(s, (row = { season: s, items: [], packs: [] }));
		return row;
	};
	for (const ep of episodes) ensure(ep.season).items.push(ep);
	for (const p of packs) ensure(p.season).packs.push(p);
	return [...by.values()].toSorted((a, b) => a.season - b.season);
}

/** Season 0 is « Specials »: a show opens on its first real season. */
export const firstSeason = (seasons: Season[]) => (seasons.find((s) => s.season > 0) ?? seasons[0])?.season;

export const watchedEp = (ep: Episode) => ep.variants.some((v) => v.status !== 'available' && v.watched);
export const ownedEp = (ep: Episode) => ep.variants.some((v) => v.status === 'downloaded');

/** The way a row is named: « Episode 1156 » on the absolute axis, else its number in its season. */
export function episodeName(ep: Episode): string {
	if (ep.absolute !== null) return `Episode ${ep.absolute}`;
	return ep.season === 0 ? `Special ${ep.episode}` : `Episode ${ep.episode}`;
}

/** The row named for an action's label: « season 2, episode 4 », « episode 1156 ». */
export function episodeWords(ep: Episode): string {
	if (ep.absolute !== null) return `episode ${ep.absolute}`;
	if (ep.season === 0) return `special ${ep.episode}`;
	return `season ${ep.season}, episode ${ep.episode}`;
}

export function seasonName(season: number): string {
	return season === 0 ? 'Specials' : `Season ${season}`;
}

/** The release language tags of the backend (`french`, `english`, `multi`, `unknown`) as the
 * search tags `format.ts` says in words. */
const TAG: Record<string, string> = { french: 'fr', english: 'en', multi: 'multi', vostfr: 'vost', vo: 'vo' };
/** The language alone, inside a sentence: « Play in French ». */
const WORD: Record<string, string> = {
	french: 'French',
	english: 'English',
	multi: 'several languages',
	vostfr: 'original with French subtitles'
};

export function languageWord(lang: string | null | undefined): string | null {
	return lang ? (WORD[lang] ?? null) : null;
}

/** « English audio (original) », « French audio (VF) ». `original` is TMDB's ISO 639-1. */
export function audioChip(lang: string, original?: string | null): string | null {
	const tag = TAG[lang];
	if (!tag) return null;
	if (original && original === tag) return `${WORD[lang]} audio (original)`;
	return languageLabel(tag, 'long');
}

/** A movie's release language, read from its SCENE name (a torrent has no language field). */
export function nameLanguage(name: string): string | null {
	const tokens = name.toLowerCase().split(/[.\s_\-[\]()]+/);
	if (tokens.includes('multi')) return 'multi';
	if (tokens.some((t) => t === 'vostfr' || t === 'subfrench')) return 'vostfr';
	if (tokens.some((t) => ['french', 'truefrench', 'vff', 'vfq', 'vfi', 'vf', 'vf2'].includes(t))) return 'french';
	return null;
}

/** A release's picture quality in words, from its name: « 1080p · HEVC ». */
export function qualityWords(name: string): string | null {
	const res = /\b(2160p|4k|1080p|720p|576p|480p)\b/i.exec(name)?.[1];
	const codec = /\b(x265|h\.?265|hevc)\b/i.test(name)
		? 'HEVC'
		: /\bav1\b/i.test(name)
			? 'AV1'
			: /\b(x264|h\.?264|avc)\b/i.test(name)
				? 'H.264'
				: null;
	const hdr = /\b(hdr10\+?|hdr|dv|dovi)\b/i.test(name) ? 'HDR' : null;
	const parts = [res ? (res.toLowerCase() === '4k' ? '2160p' : res.toLowerCase()) : null, codec, hdr].filter(Boolean);
	return parts.length ? parts.join(' · ') : null;
}

/** The file a release plays: its biggest video. */
export function mainVideo(t: TorrentView) {
	return t.files.filter((f) => isVideo(f.path)).toSorted((a, b) => b.size_bytes - a.size_bytes)[0];
}

/** Where « Play » starts with no resume point: the first episode on disk by (season, episode),
 * never `files[0]` (a pack's sample); a movie, or a pack never split: its first video. */
export function firstPlayable(
	c: CollectionDetail
): { infohash: string; idx: number; season?: number; episode?: number; absolute?: number | null } | null {
	const owned = c.episodes.filter((e) => e.episode > 0).toSorted((a, b) => a.season - b.season || a.episode - b.episode);
	if (owned[0])
		return {
			infohash: owned[0].infohash,
			idx: owned[0].file_idx,
			season: owned[0].season,
			episode: owned[0].episode,
			absolute: owned[0].absolute_episode
		};
	for (const t of c.torrents) {
		const f = t.files.find((x) => isVideo(x.path));
		if (f) return { infohash: t.infohash, idx: f.index };
	}
	return null;
}

/** The resume point: the latest unfinished item of this collection in Continue Watching. */
export function resumeOf(c: CollectionDetail, items: ContinueWatchingItem[] | undefined): ContinueWatchingItem | null {
	if (!items) return null;
	const owned = new Set(c.torrents.map((t) => t.infohash));
	return (
		items
			.filter((it) => !it.completed && !it.grabbable && owned.has(it.infohash))
			.toSorted((a, b) => Date.parse(b.last_watched_at) - Date.parse(a.last_watched_at))[0] ?? null
	);
}

/** The main action's words: « Resume S2:E4 at 32:10 », « Play S2:E5 », « Start S1:E1 », « Play ». */
export function playLabel(c: CollectionDetail, resume: ContinueWatchingItem | null, clock: (s: number) => string): string {
	if (resume) {
		const code = episodeCode(resume.season, resume.episode);
		if (resume.next_up || resume.position_seconds <= 0) return code ? `Play ${code}` : 'Play';
		return code ? `Resume ${code} at ${clock(resume.position_seconds)}` : `Resume at ${clock(resume.position_seconds)}`;
	}
	const first = firstPlayable(c);
	if (c.kind === 'tv' && typeof first?.episode === 'number') {
		return c.numbering === 'absolute' && typeof first.absolute === 'number'
			? `Start episode ${first.absolute}`
			: `Start ${episodeCode(first.season, first.episode)}`;
	}
	return 'Play';
}

/** A list in words: « English, French ». */
export const listWords = (words: string[]) => [...new Set(words)].join(', ');
