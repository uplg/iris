// Playback languages as people say them. The preferences carry ISO 639 codes (the player's
// track tags: `en`, `fra`); the releases carry the backend's tags (`french`).

export const prefsKey = (collectionId: string) => ['playback-prefs', collectionId] as const;

const names = (() => {
	try {
		return new Intl.DisplayNames(['en'], { type: 'language' });
	} catch {
		return null;
	}
})();

/** `fr` → « French »; a code the browser does not know stays as it is. */
export function languageName(code: string): string {
	try {
		return names?.of(code) ?? code;
	} catch {
		return code;
	}
}

const CODE: Record<string, string> = { french: 'fr', english: 'en' };

/** The release tags of a collection as preference codes (`multi` and `unknown` name no language). */
export function releaseCodes(tags: (string | null | undefined)[]): string[] {
	return [...new Set(tags.map((t) => (t ? CODE[t] : undefined)).filter((c): c is string => !!c))];
}
