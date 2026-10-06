// Every server read that more than one view shares, said once: its cache key and how it is
// asked, so two views never keep the same answer under two names and an invalidation reaches
// all of them. Views spread these into `createQuery` and add only their own `select`/`enabled`.
// Live values poll with `refetchInterval` (quick while something moves), never a timer.

import {
	discover,
	follows,
	library,
	me,
	metadata,
	torrents,
	type CollectionListItem,
	type LibraryResponse,
	type MediaKind,
	type TorrentView
} from '@iris/api/client';
import { queryClient } from '#lib/query.ts';

export const KEYS = {
	library: ['library'],
	collections: ['library', 'collections'],
	torrents: ['library', 'torrents'],
	collection: (id: string) => ['collection', id] as const,
	progress: (infohash: string) => ['torrent-progress', infohash] as const,
	torrent: (infohash: string) => ['torrent', infohash] as const,
	episodeContext: (infohash: string, fileIdx: number) => ['episode-context', infohash, fileIdx] as const,
	summary: ['me', 'summary'],
	continueWatching: ['continue-watching'],
	watchlist: ['watchlist'],
	history: ['history'],
	follows: ['follows'],
	recentSearches: ['recent-searches'],
	search: ['search'],
	featured: ['discover-featured'],
	forYou: ['for-you'],
	forYouPage: ['for-you-page'],
	moodBoard: (kind: MediaKind) => ['mood-board', kind] as const,
	moodResults: ['mood-results'],
	preferences: ['preferences'],
	genres: ['genres'],
	languages: ['languages'],
	playbackPrefsAll: ['playback-prefs'],
	playbackPrefs: (collectionId: string | null) => ['playback-prefs', collectionId] as const,
	tmdb: (id: number | null, kind: MediaKind | null) => ['tmdb', id, kind] as const
} as const;

/** Live progress: quick while something moves, slow otherwise. */
export const FAST = 3_000;
export const SLOW = 30_000;

const DAY = 24 * 60 * 60_000;

export const collectionsOf = (d: LibraryResponse | undefined): CollectionListItem[] => (d?.view === 'collections' ? d.items : []);
export const torrentsOf = (d: LibraryResponse | undefined): TorrentView[] => (d?.view === 'torrents' ? d.items : []);

/** A release still fetching data (not finished, not stopped). */
export const moving = (t: TorrentView) => !t.finished && t.progress_pct < 100 && (t.state === 'live' || t.state === 'initializing');

const somethingMoves = (d: LibraryResponse | undefined) => torrentsOf(d).some(moving);

export const read = {
	collections: () => ({
		queryKey: KEYS.collections,
		queryFn: () => library.list('collections'),
		// a finished download changes a title's episodes: read again sooner while one runs
		refetchInterval: () => (somethingMoves(queryClient.getQueryData<LibraryResponse>(KEYS.torrents)) ? 10_000 : 60_000)
	}),
	torrents: () => ({
		queryKey: KEYS.torrents,
		queryFn: () => library.list('torrents'),
		refetchInterval: (q: { state: { data?: LibraryResponse } }) => (somethingMoves(q.state.data) ? FAST : SLOW)
	}),
	summary: () => ({
		queryKey: KEYS.summary,
		queryFn: me.summary,
		refetchInterval: (q: { state: { data?: { downloading: number } } }) => ((q.state.data?.downloading ?? 0) > 0 ? FAST : SLOW)
	}),
	/** One release, live: quick while it fetches data, slow once it only shares. */
	torrent: (infohash: string) => ({
		queryKey: KEYS.torrent(infohash),
		queryFn: () => torrents.get(infohash),
		refetchInterval: (q: { state: { data?: TorrentView } }) => (q.state.data && !moving(q.state.data) ? SLOW : FAST)
	}),
	/** Where a file sits in its series, and the next episode's state (re-read when it may have changed). */
	episodeContext: (infohash: string, fileIdx: number) => ({
		queryKey: KEYS.episodeContext(infohash, fileIdx),
		queryFn: () => follows.episodeContext(infohash, fileIdx),
		staleTime: 5 * 60_000
	}),
	continueWatching: () => ({ queryKey: KEYS.continueWatching, queryFn: me.continueWatching }),
	watchlist: () => ({ queryKey: KEYS.watchlist, queryFn: me.watchlist, staleTime: 60_000 }),
	follows: () => ({ queryKey: KEYS.follows, queryFn: () => follows.list(), staleTime: 60_000 }),
	recentSearches: () => ({ queryKey: KEYS.recentSearches, queryFn: () => me.recentSearches(), staleTime: 60_000 }),
	preferences: () => ({ queryKey: KEYS.preferences, queryFn: me.preferences, staleTime: 5 * 60_000 }),
	genres: () => ({ queryKey: KEYS.genres, queryFn: discover.genres, staleTime: DAY }),
	languages: () => ({ queryKey: KEYS.languages, queryFn: discover.languages, staleTime: DAY }),
	/** The account's audio and subtitle choice, or a series' own when it has one. */
	playbackPrefs: (collectionId: string | null) => ({
		queryKey: KEYS.playbackPrefs(collectionId),
		queryFn: () => (collectionId ? me.seriesPlaybackPreferences(collectionId) : me.playbackPreferences()),
		staleTime: 5 * 60_000
	}),
	/** A title's TMDB metadata; ask only when the match is trusted (a wrong name is worse than none). */
	tmdb: (id: number | null | undefined, kind: MediaKind | null | undefined) => {
		const known = typeof id === 'number' ? id : null;
		return {
			queryKey: KEYS.tmdb(known, kind ?? null),
			queryFn: () => metadata.tmdb(known ?? 0, kind ?? undefined),
			enabled: known !== null,
			staleTime: 60 * 60_000
		};
	}
};

/** After a release is deleted, paused or a title hidden: what shows it is read again. */
export const refreshLibrary = () =>
	Promise.all(
		[KEYS.library, KEYS.summary, KEYS.continueWatching, KEYS.history, KEYS.watchlist].map((queryKey) =>
			queryClient.invalidateQueries({ queryKey })
		)
	);

/** A playback language saved: a series' own choice is read again; the account-wide one is
 * every series' fallback, so all of them are. */
export const playbackPrefsSaved = (collectionId: string | null) =>
	queryClient.invalidateQueries({ queryKey: collectionId ? KEYS.playbackPrefs(collectionId) : KEYS.playbackPrefsAll });
