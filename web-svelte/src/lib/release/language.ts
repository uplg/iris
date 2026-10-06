// The language a tracker's text is written in, for `lang` (a screen reader then reads it with
// the right voice). Iris does not know a tracker's language, so the text says it: French when
// its common French words outnumber its common English ones (at least three of them), else
// nothing is claimed and the page's English applies.

const FR = new Set(['le', 'la', 'les', 'des', 'une', 'du', 'et', 'est', 'pour', 'avec', 'sur', 'dans', 'qui', 'pas', 'sont', 'au', 'aux']);
const EN = new Set(['the', 'and', 'is', 'of', 'with', 'for', 'this', 'that', 'are', 'from', 'to', 'in', 'on', 'not']);

export function textLang(text: string): 'fr' | undefined {
	let fr = 0;
	let en = 0;
	for (const word of text.toLowerCase().match(/\p{L}+/gu) ?? []) {
		if (FR.has(word)) fr++;
		else if (EN.has(word)) en++;
	}
	return fr >= 3 && fr > en ? 'fr' : undefined;
}
