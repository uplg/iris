// What a search is, the one way it is written in the address: the words asked, the kind, the
// order, the audio filter and the one title kept. A link to /search?… replays it; a reload keeps it.

import type { MediaKind, SearchOpts, SortField, SortOrder } from '@iris/api/client';
import { LANGUAGE_TAGS, type LanguageTag } from '@iris/api/format';

/** `relevance` sends no `sort_by`: the backend's ranker (title and episode match, quality,
 * library demotion) owns the order. The others pass straight to the trackers. */
export type SortMode = 'relevance' | 'seeders' | 'uploaded' | 'size' | 'title';

export const SORT_MODES: readonly { value: SortMode; label: string }[] = [
	{ value: 'relevance', label: 'Best match' },
	{ value: 'seeders', label: 'Most seeders' },
	{ value: 'uploaded', label: 'Newest' },
	{ value: 'size', label: 'Smallest' },
	{ value: 'title', label: 'Name' }
];

const SORTS: Record<SortMode, { sort_by?: SortField; order?: SortOrder }> = {
	relevance: {},
	seeders: { sort_by: 'seeders', order: 'desc' },
	uploaded: { sort_by: 'uploaded', order: 'desc' },
	size: { sort_by: 'size', order: 'asc' },
	title: { sort_by: 'title', order: 'asc' }
};

export const KINDS: readonly { value: MediaKind | null; label: string }[] = [
	{ value: null, label: 'All' },
	{ value: 'movie', label: 'Movies' },
	{ value: 'tv', label: 'Series' }
];

export interface SearchState {
	/** The words searched (on submit), trimmed; '' before any search. */
	q: string;
	kind: MediaKind | null;
	sort: SortMode;
	/** Page-local audio filter (`SearchResult.language_tag`). */
	lang: LanguageTag | null;
	/** Only this TMDB title's releases (a title chosen in the Titles view). */
	title: number | null;
}

const isSort = (v: string | null): v is SortMode => !!v && v in SORTS;
const isLang = (v: string | null): v is LanguageTag => LANGUAGE_TAGS.some((t) => t.tag === v);

export function readSearch(url: { searchParams: { get(name: string): string | null } }): SearchState {
	const p = url.searchParams;
	const kind = p.get('kind');
	const sort = p.get('sort');
	const lang = p.get('lang');
	const title = Number(p.get('title'));
	return {
		q: (p.get('q') ?? '').trim(),
		kind: kind === 'movie' || kind === 'tv' ? kind : null,
		sort: isSort(sort) ? sort : 'relevance',
		lang: isLang(lang) ? lang : null,
		title: Number.isInteger(title) && title > 0 ? title : null
	};
}

/** The address of a search (defaults left out, so a plain search stays `/search?q=…`). */
export function searchHref(s: Partial<SearchState>): string {
	const p = new URLSearchParams();
	if (s.q) p.set('q', s.q);
	if (s.kind) p.set('kind', s.kind);
	if (s.sort && s.sort !== 'relevance') p.set('sort', s.sort);
	if (s.lang) p.set('lang', s.lang);
	if (s.title) p.set('title', String(s.title));
	const qs = p.toString();
	return qs ? `/search?${qs}` : '/search';
}

/** What the tracker search is asked (one page at a time). */
export function searchOpts(s: SearchState, page: number, limit: number): SearchOpts {
	return { page, limit, ...SORTS[s.sort], kind: s.kind ?? undefined, tmdb_id: s.title ?? undefined };
}
