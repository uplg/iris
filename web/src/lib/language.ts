// A language as people say it, from a track's tag or a preference's code (ISO 639-1 or -2,
// a region or not): one reading for the player, the account and a series' languages.

import { normalizeLang } from '@iris/core/subs/pick-subtitle';

const names = (() => {
	try {
		return new Intl.DisplayNames(['en'], { type: 'language' });
	} catch {
		return null;
	}
})();

/** `fr`, `fre`, `fr-FR` → « French »; a code the browser does not know reads in capitals; absent → null. */
export function languageName(tag: string | null | undefined): string | null {
	const code = normalizeLang(tag);
	if (!code) return null;
	try {
		const name = names?.of(code);
		return name && name !== code ? name : code.toUpperCase();
	} catch {
		return code.toUpperCase();
	}
}
