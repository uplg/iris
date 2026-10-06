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

/** The subtitle preference that turns them off (its value on the wire). */
export const OFF = 'off';
/** The off choice, in every picker. */
export const SUBTITLES_OFF = 'Subtitles off';
/** A file that carries no subtitles at all (not the off choice). */
export const NO_SUBTITLES = 'No subtitles';
/** No language chosen: the file decides. */
export const FILE_OWN = 'The file’s own';

/** An audio preference in words: « French », or « The file’s own ». */
export const audioWords = (v: string | null | undefined): string => (v ? (languageName(v) ?? v) : FILE_OWN);

/** A subtitle preference in words: « Subtitles off », « French », or « The file’s own ». */
export const subtitleWords = (v: string | null | undefined): string => (v === OFF ? SUBTITLES_OFF : v ? (languageName(v) ?? v) : FILE_OWN);

/** The languages a play uses, as a phrase: « audio in French, subtitles off ». A choice left
 * open is left out, or with `usual` said as « your usual audio » (a series' choice left open
 * falls back to the account's); null when there is nothing to say. */
export function languagesPhrase(p: { audio_language?: string | null; subtitle_language?: string | null }, usual = false): string | null {
	const parts: string[] = [];
	if (p.audio_language) parts.push(`audio in ${languageName(p.audio_language) ?? p.audio_language}`);
	else if (usual) parts.push('your usual audio');
	if (p.subtitle_language === OFF) parts.push('subtitles off');
	else if (p.subtitle_language) parts.push(`subtitles in ${languageName(p.subtitle_language) ?? p.subtitle_language}`);
	else if (usual) parts.push('your usual subtitles');
	return parts.length ? parts.join(', ') : null;
}
