// A release said for people: what title it is (the server's `title_match`, else its cleaned
// name), what part of it (a season, an episode), how it sounds and looks, and whether the swarm
// can deliver it. Every search surface (grid, list, details) reads these, never its own.

import type { LibraryMatch, ProviderResultMeta, SearchResult } from '@iris/api/client';
import {
	episodeCode,
	fileName,
	formatRelative,
	formatSize,
	kindWord,
	languageLabel,
	plural,
	prettySceneName,
	timeLeft
} from '@iris/api/format';
import { watchHref } from '#lib/paths.ts';
import { resumeOf } from '#lib/watched.ts';

/** A season or an episode as people say it; episode 0 is the parser's whole-season mark. */
export function partWords(season: number | null | undefined, episode: number | null | undefined, name = ''): string | null {
	if (typeof season !== 'number') return /\b(complete|integrale|int[ée]grale)\b/i.test(name) ? 'Complete series' : null;
	if (!episode) return `Season ${season}, complete`;
	return episodeCode(season, episode);
}

/** `S01E02`, `S1E2`, `S01.E02` in a release or file name; a season alone (`S02`) is episode 0. */
export function sceneMark(name: string): { season: number; episode: number } | null {
	const base = fileName(name);
	const se = /\bS(\d{1,4})[._ -]*E(\d{1,4})\b/i.exec(base);
	if (se) return { season: Number(se[1]), episode: Number(se[2]) };
	const s = /\bS(\d{1,2})\b/i.exec(base);
	return s ? { season: Number(s[1]), episode: 0 } : null;
}

/** The title a release belongs to, as a card names it. */
export function titleOf(r: Pick<SearchResult, 'title' | 'title_match'>): string {
	return r.title_match?.title ?? prettySceneName(r.title);
}

/** "Severance · Series · 2022" */
export function titleLine(r: SearchResult): string {
	const year = r.title_match?.year ?? r.year;
	return [titleOf(r), kindWord(r.title_match?.kind ?? r.kind), year].filter(Boolean).join(' · ');
}

/** The picture height a release name says (no field for it on the wire). */
export function resolution(name: string): string | null {
	const m = /\b(4320|2160|1440|1080|720|576|480)[pi]\b/i.exec(name);
	if (m) return `${m[1]}p`;
	return /\b(4k|uhd)\b/i.test(name) ? '2160p' : null;
}

const CODECS: Record<string, string> = { h264: 'H.264', hevc: 'HEVC', av1: 'AV1', vp9: 'VP9' };

export function codecWord(codec: string | null | undefined): string | null {
	return (codec && CODECS[codec]) ?? null;
}

export function seedersWords(n: number | null | undefined): string | null {
	if (typeof n !== 'number') return null;
	return plural(n, 'seeder');
}

/** A confirmed empty swarm: its pieces would never all arrive. Unknown is not dead. */
export const isDead = (seeders: number | null | undefined) => seeders === 0;
export const DEAD = 'No seeders right now';

/** What a release carries, as chips: the part, the audio, the picture, the codec. */
export function releaseChips(r: SearchResult): string[] {
	return [
		partWords(r.parsed_season, r.parsed_episode, r.title),
		languageLabel(r.language_tag, 'short'),
		resolution(r.title),
		codecWord(r.codec)
	].filter((c): c is string => !!c);
}

/** "142 seeders · 12.4 GB · torr9 · 3d ago" */
export function factsLine(r: SearchResult): string {
	return [
		seedersWords(r.seeders),
		typeof r.size_bytes === 'number' ? formatSize(r.size_bytes) : null,
		r.provider_id,
		r.uploaded_at ? formatRelative(r.uploaded_at) : null
	]
		.filter(Boolean)
		.join(' · ');
}

/** The one key of a release across trackers (two trackers may reuse an id). */
export const releaseKey = (r: Pick<SearchResult, 'provider_id' | 'external_id'>) => `${r.provider_id}:${r.external_id}`;

export const releaseHref = (r: Pick<SearchResult, 'provider_id' | 'external_id'>) =>
	`/release/${encodeURIComponent(r.provider_id)}/${encodeURIComponent(r.external_id)}`;

/** Already on disk, with the file to play: the release plays from there. */
export function ownedFile(r: SearchResult): { infohash: string; idx: number } | null {
	return r.already_in_library && r.library_infohash && typeof r.library_file_idx === 'number'
		? { infohash: r.library_infohash, idx: r.library_file_idx }
		: null;
}

/** A library match: where it leads (the exact episode asked, else its collection) and what it holds. */
export function matchTarget(m: LibraryMatch): { href: string; action: string; facts: string } {
	const resume = resumeOf(m.watch);
	if (m.episode_infohash && typeof m.episode_file_idx === 'number') {
		const code = episodeCode(m.episode_season, m.episode_number) ?? '';
		return {
			href: watchHref(m.episode_infohash, m.episode_file_idx),
			action: `${resume ? 'Resume' : 'Play'} ${code}`.trim(),
			facts: resume && resume.left !== null ? timeLeft(resume.left) : 'The episode you asked for is on disk'
		};
	}
	if (resume) {
		return {
			href: watchHref(resume.infohash, resume.fileIdx),
			action: `Resume ${resume.code ?? ''}`.trim(),
			facts: resume.left !== null ? timeLeft(resume.left) : 'In progress'
		};
	}
	const facts =
		typeof m.season_episode_count === 'number' && typeof m.episode_season === 'number'
			? `Season ${m.episode_season}: ${plural(m.season_episode_count, 'episode')} on disk`
			: m.kind === 'tv' && m.episode_count > 0
				? `${plural(m.episode_count, 'episode')} on disk`
				: m.torrent_count > 1
					? `${plural(m.torrent_count, 'release')} on disk`
					: 'On disk';
	return { href: `/collection/${m.collection_id}`, action: 'Open', facts };
}

/** How many releases of each TMDB title the loaded results hold (`title_match`). */
export function releasesByTitle(rows: readonly SearchResult[]): Map<number, number> {
	const counts = new Map<number, number>();
	for (const r of rows) {
		const id = r.title_match?.tmdb_id;
		if (typeof id === 'number') counts.set(id, (counts.get(id) ?? 0) + 1);
	}
	return counts;
}

/** The trackers that failed to answer this search. */
export const failedTrackers = (meta: readonly ProviderResultMeta[]) => meta.filter((p) => !!p.error);

/** "1 match in your library · 24 releases from 3 trackers · c411 did not answer" */
export function summary(matches: number, releases: number, meta: readonly ProviderResultMeta[]): string {
	const answered = meta.filter((p) => !p.error).length;
	const parts: string[] = [];
	if (matches) parts.push(`${plural(matches, 'match', 'matches')} in your library`);
	const rel = plural(releases, 'release');
	parts.push(answered ? `${rel} from ${plural(answered, 'tracker')}` : rel);
	const failed = failedTrackers(meta).map((p) => p.id);
	if (failed.length) parts.push(`${failed.join(', ')} did not answer`);
	return parts.join(' · ');
}
