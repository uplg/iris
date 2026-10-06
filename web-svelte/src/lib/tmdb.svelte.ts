// A title's TMDB metadata (its clean name, backdrop, overview), read only when the server
// trusts the match: a wrong backdrop or name is worse than the release's own name. Used
// where the API gives no artwork of its own (Continue Watching, the heroes, a title's page).

import { createQuery } from '@tanstack/svelte-query';
import type { MediaKind } from '@iris/api/client';
import { queryClient } from '#lib/query.ts';
import { read } from '#lib/queries.ts';

export interface TmdbRef {
	id: number | null | undefined;
	kind: MediaKind | null | undefined;
	/** The server verified the match (`tmdb_verified`), or the id is the collection's own. */
	trusted: boolean;
}

export function tmdbMeta(ref: () => TmdbRef) {
	return createQuery(
		() => {
			const r = ref();
			const q = read.tmdb(r.id, r.kind);
			return { ...q, enabled: q.enabled && r.trusted };
		},
		() => queryClient
	);
}
