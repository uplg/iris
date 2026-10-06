// The search results already read, the one place a release page finds what the search knew
// about its release (seeders, poster, its title, whether it is on disk): the details endpoint
// does not carry these.

import type { InfiniteData } from '@tanstack/svelte-query';
import type { AggregatedResults, SearchResult } from '@iris/api/client';
import { queryClient } from '#lib/query.ts';
import { KEYS } from '#lib/queries.ts';

/** Where the last search was, for the way back from a release. */
let lastSearch = '/search';
export const backToResults = () => lastSearch;
export const rememberSearch = (href: string) => (lastSearch = href);

export function findRelease(provider: string, id: string): SearchResult | null {
	for (const [, data] of queryClient.getQueriesData<InfiniteData<AggregatedResults>>({ queryKey: KEYS.search })) {
		for (const page of data?.pages ?? []) {
			const hit = page.results.find((r) => r.provider_id === provider && r.external_id === id);
			if (hit) return hit;
		}
	}
	return null;
}
