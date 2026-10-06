// The library's facts, said in words: what a title or a release is doing, how the titles filter
// and sort, which group a release belongs to. Pure functions over the API shapes, so the views
// stay markup and the rules are tested in Node.

import type { CollectionListItem, ContinueWatchingItem, HomeSummary, LibraryResponse, TorrentView } from '@iris/api/client';
import { duration, episodeCode, formatSize, percent, prettySceneName, speed } from '@iris/api/format';
import type { Tone } from '#lib/components/StatusLine.svelte';

/** The page's views, remembered per browser. */
export type View = 'titles' | 'downloads';
export const VIEWS: readonly View[] = ['titles', 'downloads'];
export const LIBRARY_VIEW_KEY = 'iris-library-view';
export const LIBRARY_SORT_KEY = 'iris-library-sort';

export const KEYS = {
	collections: ['library', 'collections'],
	torrents: ['library', 'torrents'],
	summary: ['me', 'summary'],
	continueWatching: ['continue-watching']
} as const;

/** Live progress: quick while something moves, slow otherwise (`refetchInterval`, never a timer). */
export const FAST = 3_000;
export const SLOW = 30_000;

const VIDEO_RE = /\.(mkv|mp4|webm|m4v|avi|mov|ts|mts|m2ts|wmv)$/i;

export const isVideo = (path: string) => VIDEO_RE.test(path);

/** A single-file torrent is named after its file: the release name drops the extension. */
export const releaseName = (name: string) => name.replace(VIDEO_RE, '');

export const collectionsOf = (d: LibraryResponse | undefined): CollectionListItem[] => (d?.view === 'collections' ? d.items : []);
export const torrentsOf = (d: LibraryResponse | undefined): TorrentView[] => (d?.view === 'torrents' ? d.items : []);

/** A release still fetching data (not finished, not stopped). */
export const moving = (t: TorrentView) => !t.finished && t.progress_pct < 100 && (t.state === 'live' || t.state === 'initializing');

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

// Titles

export type Kind = 'movie' | 'series' | 'anime';
export const kindOf = (c: CollectionListItem): Kind => (c.is_anime ? 'anime' : c.kind === 'tv' ? 'series' : 'movie');
const KIND_WORD: Record<Kind, string> = { movie: 'Movie', series: 'Series', anime: 'Anime' };

/** « 22 movies · 38 series · 4 anime »: what is on disk, ghosts left out. */
export function titleCounts(items: CollectionListItem[]): { total: number; text: string } {
	const live = items.filter((c) => !c.ghost);
	const n = (k: Kind) => live.filter((c) => kindOf(c) === k).length;
	return { total: live.length, text: [plural(n('movie'), 'movie'), `${n('series')} series`, `${n('anime')} anime`].join(' · ') };
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

/** The season or episode a release carries (`S4`, `S4:E2`), when its name says it. */
export function seasonOf(name: string | null | undefined): string | null {
	const m = /(?:^|[^a-z0-9])s(\d{1,2})(?:e(\d{1,3}))?(?![a-z0-9])/i.exec(name ?? '');
	if (!m) return null;
	const code = episodeCode(Number(m[1]), m[2] ? Number(m[2]) : null);
	return code?.startsWith('Season') ? `S${m[1].replace(/^0/, '')}` : code;
}

/** A title's line of facts: « Series · 4.2 GB ». */
export function titleMeta(c: CollectionListItem): string {
	return c.ghost ? KIND_WORD[kindOf(c)] : `${KIND_WORD[kindOf(c)]} · ${formatSize(c.total_size_bytes)}`;
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

const done = (t: TorrentView) => t.finished || t.progress_pct >= 100;

/** Where a release goes: fetching, needing a hand (an error, no peers, stopped, or paused by
 * its tracker's policy), or sharing what it has. */
export function groupOf(t: TorrentView): Group {
	if (t.state === 'error' || t.state === 'paused') return 'attention';
	if (done(t)) return 'seeding';
	if (t.state === 'live' && t.peers === 0 && t.download_speed_bps === 0) return 'attention';
	return 'downloading';
}

/** Seconds until a release finishes at its current speed; null when nothing moves. */
export function etaSeconds(t: TorrentView): number | null {
	if (t.download_speed_bps <= 0) return null;
	return Math.max(0, t.total_size_bytes - t.progress_bytes) / t.download_speed_bps;
}

export function ratioOf(sent: number, received: number | undefined): number | null {
	return received && received > 0 ? sent / received : null;
}

/** A release's state line, in words. */
export function releaseStatus(t: TorrentView): { tone: Tone; text: string } {
	const pct = percent(Math.min(100, Math.max(0, t.progress_pct)));
	if (t.state === 'error') return { tone: 'warn', text: t.error ? `Error · ${t.error}` : 'Error · the engine stopped this release' };
	if (t.state === 'paused') {
		if (done(t)) {
			const from = t.source_provider ? `${t.source_provider} releases never seed` : 'its tracker does not seed';
			return { tone: 'info', text: `Paused after download · ${from}` };
		}
		return { tone: 'warn', text: `Paused · ${pct}` };
	}
	if (done(t)) {
		const who = t.peers === 0 ? 'nobody downloading now' : `${plural(t.peers, 'peer')} downloading`;
		return { tone: 'ok', text: `Seeding · ${who} · ${speed(t.upload_speed_bps)} up` };
	}
	if (t.state === 'initializing') return { tone: 'busy', text: `Checking files · ${pct}` };
	if (groupOf(t) === 'attention') return { tone: 'warn', text: `Stalled · no peers · ${pct}` };
	const eta = etaSeconds(t);
	const parts = [`Downloading · ${pct}`, speed(t.download_speed_bps), plural(t.peers, 'peer')];
	if (eta !== null) parts.push(`about ${duration(eta)}`);
	return { tone: 'busy', text: parts.join(' · ') };
}

/** What a release's delete removes, named: its files, the first few by name. */
export function deleteDescription(t: TorrentView): string {
	const names = t.files.map((f) => f.path.split('/').pop() ?? f.path);
	const shown = names.slice(0, 3).join(', ');
	const more = names.length > 3 ? ` and ${names.length - 3} more` : '';
	const files = names.length ? `${plural(names.length, 'file')} (${shown}${more})` : 'its files';
	return `This removes ${files} from the server for everyone, ${formatSize(t.total_size_bytes)} in all. Seeding stops. It cannot be undone.`;
}

/** A video file's watch state for the caller: watched, partly, or untouched. */
export function watchState(w: ContinueWatchingItem | undefined): { pct: number | null; done: boolean } {
	if (!w) return { pct: null, done: false };
	if (w.completed) return { pct: 100, done: true };
	const d = w.duration_seconds ?? 0;
	return { pct: d > 0 ? Math.min(100, (w.position_seconds / d) * 100) : null, done: false };
}

export function releaseTitle(t: TorrentView, c: CollectionListItem | undefined): string {
	return c?.display_title ?? (t.name ? prettySceneName(t.name) : t.infohash);
}

/** The stat tiles' words. */
export function downloadingLine(s: HomeSummary): string {
	if (s.downloading === 0) return 'Nothing downloading';
	const eta = s.downloading_eta_seconds;
	return eta !== null && eta !== undefined
		? `${percent(s.downloading_pct)} overall · about ${duration(eta)}`
		: `${percent(s.downloading_pct)} overall`;
}
