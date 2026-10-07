// The library's facts, said in words: what a title or a release is doing, how the titles filter
// and sort, which group a release belongs to. Pure functions over the API shapes, so the views
// stay markup and the rules are tested in Node.

import type { CollectionListItem, ContinueWatchingItem, TorrentView } from '@iris/api/client';
import { STORAGE } from '#lib/storage.ts';
import {
	duration,
	episodeCode,
	fileName,
	kindLabel,
	sceneEpisode,
	formatSize,
	percent,
	plural,
	prettySceneName,
	speed,
	VIDEO_RE
} from '@iris/api/format';
import type { Tone } from '#lib/components/StatusLine.svelte';
import { watchedShare, watchWords } from '#lib/watched.ts';
import { etaSeconds, isComplete, PHASE_WORDS, phaseOf } from '#lib/torrent.ts';

/** The page's views, remembered per browser. */
export type View = 'titles' | 'downloads';
export const VIEWS: readonly View[] = ['titles', 'downloads'];
export const LIBRARY_VIEW_KEY = STORAGE.libraryView;
export const LIBRARY_SORT_KEY = STORAGE.librarySort;

/** A single-file torrent is named after its file: the release name drops the extension. */
export const releaseName = (name: string) => name.replace(VIDEO_RE, '');

// Titles

export type Kind = 'movie' | 'series' | 'anime';
export const kindOf = (c: CollectionListItem): Kind => (c.is_anime ? 'anime' : c.kind === 'tv' ? 'series' : 'movie');

/** « 64 titles · 22 movies · 38 series · 4 anime »: what is on disk, ghosts left out. */
export function titleCounts(items: CollectionListItem[]): string {
	const live = items.filter((c) => !c.ghost);
	const n = (k: Kind) => live.filter((c) => kindOf(c) === k).length;
	return [plural(live.length, 'title'), plural(n('movie'), 'movie'), `${n('series')} series`, `${n('anime')} anime`].join(' · ');
}

/** What a title's releases are doing right now, from the torrents view. */
export interface Activity {
	/** Releases still fetching. */
	fetching: TorrentView[];
	/** Something that needs a hand (an error, no peers, stopped half-way). */
	trouble: boolean;
	/** The last time anyone played one of its releases (ms), 0 if never. */
	played: number;
}

export function activityByCollection(torrents: TorrentView[]): Map<string, Activity> {
	const out = new Map<string, Activity>();
	for (const t of torrents) {
		if (!t.collection_id) continue;
		const a = out.get(t.collection_id) ?? { fetching: [], trouble: false, played: 0 };
		const g = groupOf(t);
		if (g === 'downloading') a.fetching.push(t);
		else if (g === 'attention' && !t.finished) {
			a.fetching.push(t);
			a.trouble = true;
		}
		if (t.last_played_at) a.played = Math.max(a.played, Date.parse(t.last_played_at));
		out.set(t.collection_id, a);
	}
	return out;
}

/** Overall progress of some releases, by bytes (0 to 100). */
export function bytesPct(list: TorrentView[]): number {
	const total = list.reduce((s, t) => s + t.total_size_bytes, 0);
	return total > 0 ? (list.reduce((s, t) => s + t.progress_bytes, 0) / total) * 100 : 0;
}

/** The season or episode a release carries (`Season 4`, `S4:E2`), when its name says it. */
export function seasonOf(name: string | null | undefined): string | null {
	const m = sceneEpisode(name);
	return m ? episodeCode(m.season, m.episode) : null;
}

/** A title's line of facts: « Series · 4.2 GB ». */
export function titleMeta(c: CollectionListItem): string {
	const kind = kindLabel(c.kind, c.is_anime);
	return c.ghost ? kind : `${kind} · ${formatSize(c.total_size_bytes)}`;
}

/** A title's state in words: on disk, downloading, needs a hand, or gone. */
export function titleStatus(c: CollectionListItem, a: Activity | undefined): { tone: Tone; text: string } {
	if (c.ghost) return { tone: 'info', text: 'No longer on disk' };
	if (a?.fetching.length) {
		const pct = percent(bytesPct(a.fetching));
		const what = a.fetching.length === 1 ? seasonOf(a.fetching[0].name) : plural(a.fetching.length, 'release');
		if (a.trouble) return { tone: 'warn', text: `Download stuck · ${pct}` };
		return { tone: 'busy', text: what ? `Downloading ${what} · ${pct}` : `Downloading · ${pct}` };
	}
	const watch = watchWords(c.watch, c.kind, c.episode_count);
	if (watch === 'Watched') return { tone: 'info', text: watch };
	if (watch?.startsWith('In progress')) return { tone: 'ok', text: watch };
	if (c.kind === 'tv' && c.episode_count > 0) return { tone: 'ok', text: `${plural(c.episode_count, 'episode')} on disk` };
	if (c.torrent_count > 1) return { tone: 'ok', text: `${plural(c.torrent_count, 'release')} on disk` };
	return { tone: 'ok', text: 'On disk' };
}

export type TypeFilter = 'all' | Kind;
export type ShowFilter = 'all' | 'downloading' | 'gone';
export type Sort = 'recent' | 'watched' | 'title' | 'size';
export const SORTS: readonly { value: Sort; label: string }[] = [
	{ value: 'recent', label: 'Recently added' },
	{ value: 'watched', label: 'Recently watched' },
	{ value: 'title', label: 'Title A to Z' },
	{ value: 'size', label: 'Size on disk' }
];

export interface TitleFilters {
	query: string;
	type: TypeFilter;
	show: ShowFilter;
	sort: Sort;
}

/** The titles shown: filtered, then sorted (« recent » keeps the server's order). */
export function filterTitles(items: CollectionListItem[], f: TitleFilters, activity: Map<string, Activity>): CollectionListItem[] {
	const q = f.query.trim().toLowerCase();
	const out = items.filter((c) => {
		if (f.type !== 'all' && kindOf(c) !== f.type) return false;
		if (f.show === 'downloading' && !activity.get(c.id)?.fetching.length) return false;
		if (f.show === 'gone' && !c.ghost) return false;
		if (!q) return true;
		return c.display_title.toLowerCase().includes(q) || (c.tmdb_id !== null && c.tmdb_id !== undefined && String(c.tmdb_id).includes(q));
	});
	if (f.sort === 'title') out.sort((a, b) => a.display_title.localeCompare(b.display_title));
	else if (f.sort === 'size') out.sort((a, b) => b.total_size_bytes - a.total_size_bytes);
	else if (f.sort === 'watched') out.sort((a, b) => (activity.get(b.id)?.played ?? 0) - (activity.get(a.id)?.played ?? 0));
	return out;
}

// Releases

export type Group = 'downloading' | 'attention' | 'seeding';
export const GROUPS: readonly { id: Group; title: string }[] = [
	{ id: 'downloading', title: 'Downloading' },
	{ id: 'attention', title: 'Needs attention' },
	{ id: 'seeding', title: 'Seeding' }
];

/** Where a release goes. A complete one is on disk, paused by its tracker's policy or not;
 * one that still fetches is downloading, its line saying when it is stalled or failed. Only an
 * admin, who can act on them, gets those failures apart, under « Needs attention ». */
export function groupOf(t: TorrentView, admin = false): Group {
	if (isComplete(t)) return t.state === 'error' && admin ? 'attention' : 'seeding';
	const phase = phaseOf(t);
	if (admin && (phase === 'error' || phase === 'paused' || phase === 'stalled')) return 'attention';
	return 'downloading';
}

export function ratioOf(sent: number, received: number | undefined): number | null {
	return received && received > 0 ? sent / received : null;
}

/** A release's state line, in words. */
export function releaseStatus(t: TorrentView): { tone: Tone; text: string } {
	const pct = percent(Math.min(100, Math.max(0, t.progress_pct)));
	const phase = phaseOf(t);
	const word = PHASE_WORDS[phase];
	switch (phase) {
		case 'error':
			return { tone: 'warn', text: t.error ? `${word} · ${t.error}` : word };
		case 'paused':
			if (isComplete(t)) {
				const from = t.source_provider ? `${t.source_provider} releases never seed` : 'its tracker does not seed';
				return { tone: 'info', text: `${word} after download · ${from}` };
			}
			return { tone: 'warn', text: `${word} · ${pct}` };
		case 'seeding': {
			const who = t.peers === 0 ? 'nobody downloading now' : `${plural(t.peers, 'peer')} downloading`;
			return { tone: 'ok', text: `${word} · ${who} · ${speed(t.upload_speed_bps)} up` };
		}
		case 'checking':
			return { tone: 'busy', text: `${word} · ${pct}` };
		case 'stalled':
			return { tone: 'warn', text: `${word} · no peers · ${pct}` };
		case 'downloading': {
			const eta = etaSeconds(t);
			const parts = [`${word} · ${pct}`, speed(t.download_speed_bps), plural(t.peers, 'peer')];
			if (eta !== null) parts.push(`done in about ${duration(eta)}`);
			return { tone: 'busy', text: parts.join(' · ') };
		}
	}
}

/** What a release's delete removes, named: its files, the first few by name. */
export function deleteDescription(t: TorrentView): string {
	const names = t.files.map((f) => fileName(f.path));
	const shown = names.slice(0, 3).join(', ');
	const more = names.length > 3 ? ` and ${names.length - 3} more` : '';
	const files = names.length ? `${plural(names.length, 'file')} (${shown}${more})` : 'its files';
	return `This removes ${files} from the server for everyone, ${formatSize(t.total_size_bytes)} in all. Seeding stops. It cannot be undone.`;
}

/** A video file's watch state for the caller: watched, partly, or untouched. */
export function watchState(w: ContinueWatchingItem | undefined): { pct: number | null; done: boolean } {
	if (!w) return { pct: null, done: false };
	if (w.completed) return { pct: 100, done: true };
	const share = watchedShare(w.position_seconds, w.duration_seconds);
	return { pct: share === null ? null : share * 100, done: false };
}

export function releaseTitle(t: TorrentView, c: CollectionListItem | undefined): string {
	return c?.display_title ?? (t.name ? prettySceneName(t.name) : t.infohash);
}
