// Every server read that more than one view shares, said once: its cache key and how it is
// asked, so two views never keep the same answer under two names and an invalidation reaches
// all of them. Views spread these into `createQuery` and add only their own `select`/`enabled`.
// Live values poll with `refetchInterval` (quick while something moves), never a timer.

import {
	discover,
	follows,
	library,
	livetv,
	me as meApi,
	metadata,
	progress,
	torrents,
	type CollectionListItem,
	type LibraryResponse,
	type MediaKind,
	type TorrentView
} from '@iris/api/client';
import { queryClient } from '#lib/query.ts';
import { isMoving } from '#lib/torrent.ts';

export const KEYS = {
	library: ['library'],
	collections: ['library', 'collections'],
	torrents: ['library', 'torrents'],
	collectionAll: ['collection'],
	collection: (id: string) => ['collection', id] as const,
	progressAll: ['torrent-progress'],
	progress: (infohash: string) => ['torrent-progress', infohash] as const,
	torrentAll: ['torrent'],
	torrent: (infohash: string) => ['torrent', infohash] as const,
	episodeContext: (infohash: string, fileIdx: number) => ['episode-context', infohash, fileIdx] as const,
	playStatus: (infohash: string, fileIdx: number) => ['play-status', infohash, fileIdx] as const,
	probe: (infohash: string, fileIdx: number) => ['probe', infohash, fileIdx] as const,
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
	moodBoardAll: ['mood-board'],
	moodBoard: (kind: MediaKind) => ['mood-board', kind] as const,
	moodResults: ['mood-results'],
	preferences: ['preferences'],
	genres: ['genres'],
	languages: ['languages'],
	playbackPrefsAll: ['playback-prefs'],
	playbackPrefs: (collectionId: string | null) => ['playback-prefs', collectionId] as const,
	tmdb: (id: number | null, kind: MediaKind | null) => ['tmdb', id, kind] as const,
	liveCountries: ['livetv', 'countries'],
	liveChannels: (country: string) => ['livetv', 'channels', country] as const,
	liveEpg: (country: string) => ['livetv', 'epg-now', country] as const
} as const;

/** Live progress: quick while something moves, slow otherwise. */
export const FAST = 3_000;
export const SLOW = 30_000;

const DAY = 24 * 60 * 60_000;

export const collectionsOf = (d: LibraryResponse | undefined): CollectionListItem[] => (d?.view === 'collections' ? d.items : []);
export const torrentsOf = (d: LibraryResponse | undefined): TorrentView[] => (d?.view === 'torrents' ? d.items : []);

const somethingMoves = (d: LibraryResponse | undefined) => torrentsOf(d).some(isMoving);

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
	collection: (id: string) => ({ queryKey: KEYS.collection(id), queryFn: () => library.collection(id) }),
	summary: () => ({
		queryKey: KEYS.summary,
		queryFn: meApi.summary,
		refetchInterval: (q: { state: { data?: { downloading: number } } }) => ((q.state.data?.downloading ?? 0) > 0 ? FAST : SLOW)
	}),
	/** The person's positions in a release's files. Read once per release (episode rows of one
	 * release share it), not again on each return to the tab: it changes only by watching, and
	 * the watch page polls it while it plays. */
	progress: (infohash: string) => ({
		queryKey: KEYS.progress(infohash),
		queryFn: () => progress.forTorrent(infohash),
		staleTime: 5 * 60_000,
		refetchOnWindowFocus: false
	}),
	/** One release, live: quick while it fetches data, slow once it only shares. */
	torrent: (infohash: string) => ({
		queryKey: KEYS.torrent(infohash),
		queryFn: () => torrents.get(infohash),
		refetchInterval: (q: { state: { data?: TorrentView } }) => (q.state.data && !isMoving(q.state.data) ? SLOW : FAST)
	}),
	/** Where a file sits in its series, and the next episode's state (re-read when it may have changed). */
	episodeContext: (infohash: string, fileIdx: number) => ({
		queryKey: KEYS.episodeContext(infohash, fileIdx),
		queryFn: () => follows.episodeContext(infohash, fileIdx),
		staleTime: 5 * 60_000
	}),
	continueWatching: () => ({ queryKey: KEYS.continueWatching, queryFn: meApi.continueWatching }),
	watchlist: () => ({ queryKey: KEYS.watchlist, queryFn: meApi.watchlist, staleTime: 60_000 }),
	follows: () => ({ queryKey: KEYS.follows, queryFn: () => follows.list(), staleTime: 60_000 }),
	recentSearches: () => ({ queryKey: KEYS.recentSearches, queryFn: () => meApi.recentSearches(), staleTime: 60_000 }),
	forYou: () => ({ queryKey: KEYS.forYou, queryFn: meApi.forYou, staleTime: 60_000 }),
	forYouPage: () => ({ queryKey: KEYS.forYouPage, queryFn: meApi.forYouPage, staleTime: 60_000 }),
	/** The tracker's own picks: asked only when there is nothing of one's own to show. */
	featured: () => ({ queryKey: KEYS.featured, queryFn: discover.featured, staleTime: 5 * 60_000 }),
	moodBoard: (kind: MediaKind) => ({ queryKey: KEYS.moodBoard(kind), queryFn: () => meApi.moodBoard(kind) }),
	moodResults: (mood: string, kind: MediaKind) => ({
		queryKey: [...KEYS.moodResults, mood, kind],
		queryFn: () => meApi.moodResults(mood, kind)
	}),
	liveCountries: () => ({ queryKey: KEYS.liveCountries, queryFn: () => livetv.countries(), staleTime: DAY }),
	liveChannels: (country: string) => ({
		queryKey: KEYS.liveChannels(country),
		queryFn: () => livetv.channels(country),
		staleTime: 10 * 60_000
	}),
	/** The guide's now and next; each view polls it at its own pace (`refetchInterval`). */
	liveEpg: (country: string) => ({ queryKey: KEYS.liveEpg(country), queryFn: () => livetv.epgNow(country) }),
	preferences: () => ({ queryKey: KEYS.preferences, queryFn: meApi.preferences, staleTime: 5 * 60_000 }),
	genres: () => ({ queryKey: KEYS.genres, queryFn: discover.genres, staleTime: DAY }),
	languages: () => ({ queryKey: KEYS.languages, queryFn: discover.languages, staleTime: DAY }),
	/** The account's audio and subtitle choice, or a series' own when it has one. */
	playbackPrefs: (collectionId: string | null) => ({
		queryKey: KEYS.playbackPrefs(collectionId),
		queryFn: () => (collectionId ? meApi.seriesPlaybackPreferences(collectionId) : meApi.playbackPreferences()),
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

/** After a release is deleted, paused, grabbed or a title hidden: what shows it is read again,
 * its title's page and its own included (only what is on screen is asked at once). */
export const refreshLibrary = () =>
	Promise.all(
		[KEYS.library, KEYS.collectionAll, KEYS.torrentAll, KEYS.summary, KEYS.continueWatching, KEYS.history, KEYS.watchlist].map((queryKey) =>
			queryClient.invalidateQueries({ queryKey })
		)
	);

/** A playback language saved: a series' own choice is read again; the account-wide one is
 * every series' fallback, so all of them are. */
export const playbackPrefsSaved = (collectionId: string | null) =>
	queryClient.invalidateQueries({ queryKey: collectionId ? KEYS.playbackPrefs(collectionId) : KEYS.playbackPrefsAll });
