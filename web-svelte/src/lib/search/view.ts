// How the results are laid out, remembered per browser: TMDB titles (the default), a poster
// per release, or a list of releases.

import { stored, text } from '#lib/stored.ts';
import { STORAGE } from '#lib/storage.ts';

export type ResultsView = 'titles' | 'grid' | 'list';

/** To move into src/env.ts `STORAGE` with the other keys. */
export const SEARCH_VIEW_KEY = STORAGE.searchView;

export const VIEWS: readonly { value: ResultsView; label: string }[] = [
	{ value: 'titles', label: 'Titles' },
	{ value: 'grid', label: 'Grid' },
	{ value: 'list', label: 'List' }
];

export const keptView = stored<ResultsView>(SEARCH_VIEW_KEY, 'titles', text<ResultsView>(['titles', 'grid', 'list']));
