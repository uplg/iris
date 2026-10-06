// Search answers for the screens' tests, as the backend sends them.

import type { AggregatedResults, SearchResult, TitleCard } from '@iris/api/client';

export function release(over: Partial<SearchResult> = {}): SearchResult {
	return {
		provider_id: 'torr9',
		external_id: '1',
		title: 'Severance.S02.MULTi.1080p.WEB.H265-GRP',
		seeders: 142,
		leechers: 3,
		size_bytes: 12 * 1024 ** 3,
		language_tag: 'multi',
		codec: 'hevc',
		parsed_season: 2,
		parsed_episode: 0,
		kind: 'tv',
		poster_url: null,
		title_match: { tmdb_id: 95396, kind: 'tv', title: 'Severance', year: 2022 },
		...over
	};
}

export function answer(results: SearchResult[], over: Partial<AggregatedResults> = {}): AggregatedResults {
	return {
		results,
		providers: [{ id: 'torr9', current_page: 1, limit: 25, total_count: results.length, total_pages: 1 }],
		library_matches: [],
		parsed_query: null,
		...over
	};
}

export const severance: TitleCard = { tmdb_id: 95396, kind: 'tv', title: 'Severance', year: 2022, poster_url: null, collection_id: null };
