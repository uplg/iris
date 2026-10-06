// A title's TMDB metadata (its clean name, backdrop, overview), read only when the server
// trusts the match: a wrong backdrop or name is worse than the release's own name. Used
// where the API gives no artwork of its own (Continue Watching, the heroes).

import { createQuery } from '@tanstack/svelte-query';
import { metadata, type MediaKind } from '@iris/api/client';

export interface TmdbRef {
	id: number | null | undefined;
	kind: MediaKind | null | undefined;
	/** The server verified the match (`tmdb_verified`), or the id is the collection's own. */
	trusted: boolean;
}

export function tmdbMeta(ref: () => TmdbRef) {
	return createQuery(() => {
		const r = ref();
		const id = typeof r.id === 'number' ? r.id : null;
		return {
			queryKey: ['tmdb', id, r.kind ?? null],
			queryFn: () => metadata.tmdb(id ?? 0, r.kind ?? undefined),
			enabled: id !== null && r.trusted,
			staleTime: 5 * 60_000
		};
	});
}
