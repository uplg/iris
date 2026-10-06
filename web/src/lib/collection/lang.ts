// Playback languages as people say them. The preferences carry ISO 639 codes (the player's
// track tags: `en`, `fra`); the releases carry the backend's tags (`french`).

const CODE: Record<string, string> = { french: 'fr', english: 'en' };

/** The release tags of a collection as preference codes (`multi` and `unknown` name no language). */
export function releaseCodes(tags: (string | null | undefined)[]): string[] {
	return [...new Set(tags.map((t) => (t ? CODE[t] : undefined)).filter((c): c is string => !!c))];
}
