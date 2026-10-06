import { afterEach, describe, expect, it } from 'vitest';
import { queryClient } from '#lib/query.ts';
import { KEYS } from '#lib/queries.ts';
import { preferencesSaved } from './preferences.ts';

describe('preferences saved', () => {
	afterEach(() => queryClient.clear());

	it('replace the cached ones, and every suggestion they order is read again (the mood board too)', () => {
		queryClient.setQueryData(KEYS.moodBoard('movie'), { tiles: [] });
		queryClient.setQueryData(KEYS.forYou, { shelves: [] });
		const saved = { languages: ['french'], genres: [18], include_anime: false, onboarding_completed: true };
		preferencesSaved(saved);
		expect(queryClient.getQueryData(KEYS.preferences)).toEqual(saved);
		expect(queryClient.getQueryState(KEYS.moodBoard('movie'))?.isInvalidated).toBe(true);
		expect(queryClient.getQueryState(KEYS.forYou)?.isInvalidated).toBe(true);
	});
});
