// What « For You » is tuned by, for the Account page and the first-run onboarding: the part a
// person picks, and what follows a save (the suggestions it shapes are read again).

import type { Preferences } from '@iris/api/client';
import { queryClient } from '#lib/query.ts';
import { KEYS } from '#lib/queries.ts';

/** The part of the preferences a person picks (onboarding_completed is the app's). */
export type Picks = Pick<Preferences, 'languages' | 'genres' | 'include_anime'>;

export const NO_PICKS: Picks = { languages: [], genres: [], include_anime: false };

export const picksOf = (p: Preferences): Picks => ({ languages: [...p.languages], genres: [...p.genres], include_anime: p.include_anime });

/** `list` with `v` added, or taken out when it was there. */
export function toggled<T>(list: readonly T[], v: T): T[] {
	return list.includes(v) ? list.filter((x) => x !== v) : [...list, v];
}

/** The server kept these preferences: they replace the cached ones, the suggestions follow. */
export function preferencesSaved(saved: Preferences): void {
	queryClient.setQueryData(KEYS.preferences, saved);
	for (const queryKey of [KEYS.forYou, KEYS.forYouPage, KEYS.moodResults]) void queryClient.invalidateQueries({ queryKey });
}
