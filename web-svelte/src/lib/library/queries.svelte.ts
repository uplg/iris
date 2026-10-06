// The library's server values, one query each, shared by both views (the cache dedupes them).
// Live progress is the queries' own `refetchInterval`: quick while a release moves, slow
// otherwise. Called while a component initialises.

import { createQuery } from '@tanstack/svelte-query';
import { library, me, type LibraryResponse } from '@iris/api/client';
import { queryClient } from '#lib/query.ts';
import { FAST, KEYS, SLOW, moving, torrentsOf } from './model.ts';

const somethingMoves = (d: LibraryResponse | undefined) => torrentsOf(d).some(moving);

export const collectionsQuery = () =>
	createQuery(() => ({
		queryKey: KEYS.collections,
		queryFn: () => library.list('collections'),
		// a finished download changes a title's episodes: read again sooner while one runs
		refetchInterval: () => (somethingMoves(queryClient.getQueryData<LibraryResponse>(KEYS.torrents)) ? 10_000 : 60_000)
	}));

export const torrentsQuery = () =>
	createQuery(() => ({
		queryKey: KEYS.torrents,
		queryFn: () => library.list('torrents'),
		refetchInterval: (q) => (somethingMoves(q.state.data) ? FAST : SLOW)
	}));

export const summaryQuery = () =>
	createQuery(() => ({
		queryKey: KEYS.summary,
		queryFn: me.summary,
		refetchInterval: (q) => ((q.state.data?.downloading ?? 0) > 0 ? FAST : SLOW)
	}));

export const continueWatchingQuery = () =>
	createQuery(() => ({
		queryKey: KEYS.continueWatching,
		queryFn: me.continueWatching
	}));

/** After a release is deleted or a title hidden: what shows it is read again. */
export const refreshLibrary = () =>
	Promise.all([
		queryClient.invalidateQueries({ queryKey: ['library'] }),
		queryClient.invalidateQueries({ queryKey: KEYS.summary }),
		queryClient.invalidateQueries({ queryKey: KEYS.continueWatching })
	]);
