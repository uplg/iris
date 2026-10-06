// What « For You » is tuned by, read once for the Account page and the first-run onboarding:
// my choices, and what can be chosen (served by the backend, never listed here).

import { discover, me, type Preferences } from '@iris/api/client';

const DAY = 24 * 60 * 60_000;

export const preferencesQuery = () => ({ queryKey: ['preferences'], queryFn: me.preferences, staleTime: 5 * 60_000 });
export const genresQuery = () => ({ queryKey: ['genres'], queryFn: discover.genres, staleTime: DAY });
export const languagesQuery = () => ({ queryKey: ['languages'], queryFn: discover.languages, staleTime: DAY });

/** The part of the preferences a person picks (onboarding_completed is the app's). */
export type Picks = Pick<Preferences, 'languages' | 'genres' | 'include_anime'>;

export const picksOf = (p: Preferences): Picks => ({ languages: [...p.languages], genres: [...p.genres], include_anime: p.include_anime });

/** `list` with `v` added, or taken out when it was there. */
export function toggled<T>(list: readonly T[], v: T): T[] {
	return list.includes(v) ? list.filter((x) => x !== v) : [...list, v];
}
