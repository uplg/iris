// The watch page's side panel and episode context, framework-free: the season's episodes (one
// row per episode, whichever torrent holds it) or the torrent's own video files, the language
// to prefer, and what to search for when a release is dead.

import type { AvailableEpisodeEntry, CollectionEpisodeEntry, FileEntry, FileProgressEntry } from '@iris/api/client';
import { episodeCode, formatSize, prettySceneName } from '@iris/api/format';

export interface SideRow {
	key: string;
	infohash: string;
	fileIdx: number;
	/** « S2:E1 » for an episode, the file name for a raw file. */
	primary: string;
	/** The language (episodes) or the size (files). */
	secondary: string;
	mono: boolean;
	watched: boolean;
	watchedPct: number | null;
	active: boolean;
	/** Discovered, not downloaded yet: the row offers « Grab and play » (infohash empty). */
	grab?: { season: number; episode: number; language: string | null };
	season?: number;
	episode?: number;
}

export function watchedPctOf(p?: FileProgressEntry): number | null {
	return p && p.duration_seconds && p.duration_seconds > 0 ? Math.min(100, (p.position_seconds / p.duration_seconds) * 100) : null;
}

const known = (l: string | null | undefined): l is string => !!l && l !== 'unknown';

/** The language to prefer among an episode's offers: the playing file's, else the series'
 * dominant owned language (the prepare-next rule, server-side too). */
export function currentLanguage(current: CollectionEpisodeEntry | undefined, episodes: CollectionEpisodeEntry[]): string | null {
	if (known(current?.language)) return current.language;
	const counts = new Map<string, number>();
	for (const e of episodes) if (known(e.language)) counts.set(e.language, (counts.get(e.language) ?? 0) + 1);
	let best: string | null = null;
	let bestCount = 0;
	for (const [lang, count] of counts) {
		if (count > bestCount) {
			best = lang;
			bestCount = count;
		}
	}
	return best;
}

const code = (s: number, e: number) => episodeCode(s, e) ?? `S${s}:E${e}`;
const order = (r: SideRow) => (r.season ?? 0) * 100_000 + (r.episode ?? 0);

function byEpisode<T extends { season: number; episode: number }>(list: T[]): Map<string, T[]> {
	const m = new Map<string, T[]>();
	for (const x of list) {
		const k = `${x.season}:${x.episode}`;
		const l = m.get(k);
		if (l) l.push(x);
		else m.set(k, [x]);
	}
	return m;
}

export interface SideInput {
	infohash: string;
	fileIdx: number;
	isTvCollection: boolean;
	episodes: CollectionEpisodeEntry[];
	available: AvailableEpisodeEntry[];
	videoFiles: FileEntry[];
	progressByFile: Map<number, FileProgressEntry>;
}

/** Whether the panel lists the collection's episodes (else the torrent's files). */
export const listsEpisodes = (i: Pick<SideInput, 'isTvCollection' | 'episodes' | 'available'>) =>
	i.isTvCollection && (i.episodes.length > 0 || i.available.length > 0);

/**
 * The rows. A TV collection lists the current season's episodes (all seasons when the playing
 * file is not indexed yet): one row per episode, whichever file is playing, else the preferred
 * language, else Multi, else the first; an episode on disk never also shows a « grab the other
 * language » row. Anything else lists the torrent's video files.
 */
export function sideRows(i: SideInput): SideRow[] {
	if (listsEpisodes(i)) {
		const current = i.episodes.find((e) => e.infohash === i.infohash && e.file_idx === i.fileIdx);
		const season = current?.season ?? null;
		const lang = currentLanguage(current, i.episodes);
		const inSeason = <T extends { season: number }>(l: T[]) => (season !== null ? l.filter((x) => x.season === season) : l);
		const owned = byEpisode(inSeason(i.episodes));
		const downloaded = [...owned.values()].map((variants): SideRow => {
			const e =
				variants.find((v) => v.infohash === i.infohash && v.file_idx === i.fileIdx) ||
				(lang && variants.find((v) => v.language === lang)) ||
				variants.find((v) => v.language === 'multi') ||
				variants[0];
			const prog = e.infohash === i.infohash ? i.progressByFile.get(e.file_idx) : undefined;
			return {
				key: `dl:${e.infohash}:${e.file_idx}`,
				infohash: e.infohash,
				fileIdx: e.file_idx,
				primary: code(e.season, e.episode),
				secondary: known(e.language) ? e.language : '',
				mono: false,
				watched: e.watched || !!prog?.completed,
				watchedPct: watchedPctOf(prog),
				active: e.infohash === i.infohash && e.file_idx === i.fileIdx,
				season: e.season,
				episode: e.episode
			};
		});
		const unowned = inSeason(i.available).filter((a) => !owned.has(`${a.season}:${a.episode}`));
		const discovered = [...byEpisode(unowned).values()].map((variants): SideRow => {
			const a = (lang && variants.find((v) => v.language === lang)) || variants.find((v) => v.language === 'multi') || variants[0];
			return {
				key: `av:${a.season}:${a.episode}`,
				infohash: '',
				fileIdx: -1,
				primary: code(a.season, a.episode),
				secondary: known(a.language) ? a.language : '',
				mono: false,
				watched: false,
				watchedPct: null,
				active: false,
				grab: { season: a.season, episode: a.episode, language: a.language ?? null },
				season: a.season,
				episode: a.episode
			};
		});
		return [...downloaded, ...discovered].toSorted((a, b) => order(a) - order(b));
	}
	return i.videoFiles.map((f) => {
		const prog = i.progressByFile.get(f.index);
		return {
			key: `f:${f.index}`,
			infohash: i.infohash,
			fileIdx: f.index,
			primary: f.path.split('/').pop() ?? f.path,
			secondary: formatSize(f.size_bytes),
			mono: true,
			watched: !!prog?.completed,
			watchedPct: watchedPctOf(prog),
			active: f.index === i.fileIdx
		};
	});
}

/** What to search when the release is dead: the series and the episode, else the title, else
 * the cleaned-up release name (its SCENE name would find the same corpse). */
export function retrySearchQuery(
	title: string | null | undefined,
	current: { season: number; episode: number } | undefined,
	releaseName: string | null | undefined
): string {
	const pad = (n: number) => String(n).padStart(2, '0');
	if (title && current) return `${title} S${pad(current.season)}E${pad(current.episode)}`;
	if (title) return title;
	return prettySceneName(releaseName ?? '');
}
