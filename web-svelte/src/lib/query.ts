// Server state: TanStack Query, the one cache every view reads (polling is `refetchInterval`,
// never a timer of our own). `loadable` shapes a query for `Loaded` / `Group`: first load,
// failure with nothing known, a value kept but old.

import { QueryClient } from '@tanstack/svelte-query';

export const queryClient = new QueryClient({
	defaultOptions: {
		queries: {
			// a value read in the last half minute is fresh enough to show without asking again
			staleTime: 30_000,
			// a 4xx is an answer, not a blip: retry only what may pass next time
			retry: (count, e) => count < 2 && !(e instanceof Error && 'status' in e && Number(e.status) < 500),
			refetchOnWindowFocus: true
		}
	}
});

/** What a view needs to show a server value honestly. */
export interface Loadable {
	/** Only the first load: a refresh keeps showing what is known. */
	loading: boolean;
	/** Nothing known and the last ask failed: an error to show, never an empty list. */
	failed: boolean;
	error: unknown;
	/** Something is shown, but the last ask failed: said as such. */
	stale: boolean;
	/** When the shown value arrived (ms). */
	at: number;
	refresh(): Promise<unknown>;
}

interface QueryLike {
	isPending: boolean;
	isError: boolean;
	error: unknown;
	data: unknown;
	dataUpdatedAt: number;
	refetch(): Promise<unknown>;
}

/** A query as a Loadable (reads stay reactive: these are getters on the query). */
export function loadable(q: QueryLike): Loadable {
	return {
		get loading() {
			return q.isPending;
		},
		get failed() {
			return q.isError && q.data === undefined;
		},
		get error() {
			return q.error;
		},
		get stale() {
			return q.isError && q.data !== undefined;
		},
		get at() {
			return q.dataUpdatedAt;
		},
		refresh: () => q.refetch()
	};
}
