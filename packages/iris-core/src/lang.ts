// Language tags as releases, ffprobe and browsers spell them, folded to one code per language.

/**
 * ISO 639-2 (bibliographic and terminologic) to 639-1 for the languages
 * releases actually tag; anything else is lowercased with a region
 * suffix stripped. ffprobe hands back whatever the muxer wrote, and
 * `fre` / `fra` / `fr` all mean French, across releases of one series.
 */
const ISO_639_2_TO_1: Record<string, string> = {
	fre: 'fr',
	fra: 'fr',
	eng: 'en',
	ger: 'de',
	deu: 'de',
	spa: 'es',
	ita: 'it',
	por: 'pt',
	dut: 'nl',
	nld: 'nl',
	jpn: 'ja',
	kor: 'ko',
	chi: 'zh',
	zho: 'zh',
	rus: 'ru',
	ara: 'ar',
	pol: 'pl',
	tur: 'tr',
	swe: 'sv',
	nor: 'no',
	dan: 'da',
	fin: 'fi',
	cze: 'cs',
	ces: 'cs',
	gre: 'el',
	ell: 'el',
	hun: 'hu',
	rum: 'ro',
	ron: 'ro',
	ukr: 'uk',
	heb: 'he',
	hin: 'hi',
	tha: 'th',
	vie: 'vi',
	ind: 'id',
	may: 'ms',
	msa: 'ms',
	per: 'fa',
	fas: 'fa',
	cat: 'ca',
	baq: 'eu',
	eus: 'eu',
	glg: 'gl',
	slo: 'sk',
	slk: 'sk',
	slv: 'sl',
	hrv: 'hr',
	srp: 'sr',
	bul: 'bg',
	lit: 'lt',
	lav: 'lv',
	est: 'et',
	ice: 'is',
	isl: 'is',
	tgl: 'tl',
	fil: 'tl'
};

/** `null` for absent / unknown (`und`) tags. */
export function normalizeLang(code: string | null | undefined): string | null {
	if (!code) return null;
	const base = code.trim().toLowerCase().split(/[-_]/)[0] ?? '';
	if (!base || base === 'und') return null;
	return ISO_639_2_TO_1[base] ?? base;
}
