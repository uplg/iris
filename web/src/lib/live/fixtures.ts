// Live TV as the backend serves it, for the tests: the countries iptv-org carries, France's
// free-to-air with a guide, a country of several hundred channels without one.

import type { LiveChannel, LiveCountry, LiveNowNext } from '@iris/api/client';

const flag = (code: string) => String.fromCodePoint(...[...code.toUpperCase()].map((ch) => 0x1f1e6 + ch.charCodeAt(0) - 65));

const NAMES: [string, string, number][] = [
	['ad', 'Andorra', 4],
	['ae', 'United Arab Emirates', 61],
	['af', 'Afghanistan', 38],
	['al', 'Albania', 52],
	['ar', 'Argentina', 166],
	['at', 'Austria', 47],
	['au', 'Australia', 98],
	['ba', 'Bosnia and Herzegovina', 41],
	['be', 'Belgium', 63],
	['bg', 'Bulgaria', 37],
	['br', 'Brazil', 211],
	['ca', 'Canada', 154],
	['ch', 'Switzerland', 58],
	['cl', 'Chile', 120],
	['cn', 'China', 245],
	['co', 'Colombia', 98],
	['cz', 'Czech Republic', 33],
	['de', 'Germany', 172],
	['dk', 'Denmark', 19],
	['dz', 'Algeria', 30],
	['ec', 'Ecuador', 71],
	['eg', 'Egypt', 44],
	['es', 'Spain', 236],
	['fi', 'Finland', 12],
	['fr', 'France', 40],
	['gb', 'United Kingdom', 180],
	['gr', 'Greece', 83],
	['hk', 'Hong Kong', 57],
	['hr', 'Croatia', 22],
	['hu', 'Hungary', 31],
	['id', 'Indonesia', 133],
	['ie', 'Ireland', 15],
	['il', 'Israel', 26],
	['in', 'India', 288],
	['iq', 'Iraq', 94],
	['ir', 'Iran', 120],
	['it', 'Italy', 251],
	['jp', 'Japan', 39],
	['kr', 'South Korea', 70],
	['lb', 'Lebanon', 28],
	['lu', 'Luxembourg', 9],
	['ma', 'Morocco', 21],
	['mx', 'Mexico', 199],
	['nl', 'Netherlands', 71],
	['no', 'Norway', 14],
	['nz', 'New Zealand', 25],
	['pe', 'Peru', 147],
	['ph', 'Philippines', 66],
	['pk', 'Pakistan', 72],
	['pl', 'Poland', 92],
	['pt', 'Portugal', 54],
	['re', 'Réunion', 6],
	['ro', 'Romania', 78],
	['rs', 'Serbia', 64],
	['ru', 'Russia', 290],
	['sa', 'Saudi Arabia', 48],
	['se', 'Sweden', 24],
	['sn', 'Senegal', 13],
	['th', 'Thailand', 61],
	['tn', 'Tunisia', 18],
	['tr', 'Turkey', 210],
	['ua', 'Ukraine', 140],
	['us', 'United States', 300],
	['vn', 'Vietnam', 104]
];

export const COUNTRIES: LiveCountry[] = NAMES.map(([code, name, channel_count]) => ({
	code,
	name,
	flag: flag(code),
	channel_count
}));

/** A logo of one colour with a word on it, as a data URL (no network in a test). */
function logo(word: string, ground: string, ink: string): string {
	const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="120" height="60" viewBox="0 0 120 60"><rect x="6" y="6" width="108" height="48" rx="8" fill="${ground}"/><text x="60" y="38" font-family="Arial" font-weight="700" font-size="20" text-anchor="middle" fill="${ink}">${word}</text></svg>`;
	return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

const channel = (over: Partial<LiveChannel> & Pick<LiveChannel, 'id' | 'name'>): LiveChannel => ({
	logo_url: null,
	logo_origin: null,
	categories: [],
	quality: 1080,
	geo_blocked: false,
	not_24_7: false,
	tnt_number: null,
	...over
});

const TNT: [number, string, string, string][] = [
	[1, 'TF1', '#0a3d91', '#ffffff'],
	[2, 'France 2', '#e30613', '#ffffff'],
	[3, 'France 3', '#1d4fa0', '#ffffff'],
	[4, 'France 4', '#111111', '#ffffff'],
	[5, 'France 5', '#2f9e44', '#ffffff'],
	[6, 'M6', '#f6f6f6', '#c8102e'],
	[7, 'Arte', '#ffffff', '#f26522'],
	[8, 'LCP', '#ffffff', '#0b2a5b'],
	[9, 'W9', '#151515', '#ffd400'],
	[10, 'TMC', '#ffffff', '#c3002f'],
	[11, 'TFX', '#ffffff', '#3a3a3a'],
	[12, 'Gulli', '#7fc31c', '#ffffff'],
	[13, 'BFM TV', '#002f6c', '#ffffff'],
	[14, 'CNews', '#ffffff', '#ed1c24'],
	[15, 'LCI', '#ffffff', '#1a1a1a'],
	[16, 'franceinfo', '#ffd200', '#1a1a1a'],
	[17, 'CStar', '#ffffff', '#e2007a'],
	[18, 'T18', '#ffffff', '#1a1a1a'],
	[19, 'NOVO19', '#151515', '#ffffff'],
	[20, 'TF1 Séries Films', '#ffffff', '#0a3d91'],
	[21, 'L’Équipe', '#e30613', '#ffffff'],
	[22, '6ter', '#ffffff', '#4b2e83'],
	[23, 'RMC Story', '#ffffff', '#0072bc'],
	[24, 'RMC Découverte', '#002244', '#ffffff'],
	[25, 'Chérie 25', '#ffffff', '#c2185b']
];

const FR_EXTRA: [string, string, Partial<LiveChannel>][] = [
	['France 24', 'News', { logo_url: logo('F24', '#00a0e0', '#ffffff') }],
	['TV5Monde', 'General', { logo_url: logo('TV5', '#ffffff', '#0f2a4a') }],
	['Euronews', 'News', { logo_url: logo('euronews', '#ffffff', '#003a70'), geo_blocked: true }],
	['Africanews', 'News', {}],
	['Museum TV', 'Culture', { logo_url: logo('MUSEUM', '#151515', '#ffffff') }],
	['Mezzo', 'Music', { not_24_7: true }],
	['Trace Urban', 'Music', { logo_url: logo('TRACE', '#000000', '#ffd400'), unreachable: true }],
	['NRJ Hits', 'Music', { logo_url: logo('NRJ', '#e2001a', '#ffffff') }],
	['Équidia', 'Sports', { logo_url: logo('équidia', '#ffffff', '#00703c') }],
	['Ciné Nanar', 'Movies', {}],
	['Canal+ Clair', 'Entertainment', { logo_url: logo('CANAL+', '#000000', '#ffffff'), geo_blocked: true, unreachable: true }],
	['RFI TV', 'News', {}],
	['BFM Business', 'Business', { logo_url: logo('BFM B', '#002f6c', '#ffffff') }],
	['Télé Grenoble', 'Local', {}],
	['KTO', 'Religious', { logo_url: logo('KTO', '#ffffff', '#6a1b9a') }]
];

const slug = (s: string) =>
	s
		.normalize('NFD')
		.replace(/\p{M}/gu, '')
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '');

/** France: the twenty-five free-to-air channels by number, then fifteen more by category;
 * some without a logo, one possibly blocked, one not answering. */
export const FRANCE: LiveChannel[] = [
	...TNT.map(([n, name, ground, ink]) =>
		channel({
			id: slug(name),
			name,
			tnt_number: n,
			categories: ['General'],
			// a few free-to-air channels without a logo: the letter stands in
			logo_url: n === 18 || n === 19 ? null : logo(name.length > 8 ? name.slice(0, 8) : name, ground, ink)
		})
	),
	...FR_EXTRA.map(([name, cat, over]) => channel({ id: slug(name), name, categories: [cat], ...over }))
];

const PROGRAMMES = [
	'Le journal de 20 h',
	'Koh-Lanta, la légende',
	'Des chiffres et des lettres',
	'C dans l’air',
	'Plus belle la vie, encore plus belle',
	'Le Grand Échiquier',
	'Les Simpson',
	'Questions pour un champion',
	'Envoyé spécial',
	'Un si grand soleil',
	'La France a un incroyable talent',
	'Thalassa',
	'Météo',
	'Les Experts : Manhattan',
	'Faites entrer l’accusé'
];

/** The guide at `now`: what France's channels show, each part-way through its programme. */
export function franceGuide(now: number): LiveNowNext[] {
	const min = 60_000;
	return FRANCE.filter((c) => !c.unreachable && c.name !== 'Africanews' && c.name !== 'Télé Grenoble').map((c, i) => {
		const start = now - (5 + ((i * 17) % 70)) * min;
		const stop = now + (5 + ((i * 23) % 80)) * min;
		return {
			channel_id: c.id,
			now: {
				title: PROGRAMMES[i % PROGRAMMES.length],
				start: new Date(start).toISOString(),
				stop: new Date(stop).toISOString()
			},
			next: {
				title: PROGRAMMES[(i + 5) % PROGRAMMES.length],
				start: new Date(stop).toISOString(),
				stop: new Date(stop + 45 * min).toISOString()
			}
		};
	});
}

const US_CATEGORIES = [
	'News',
	'Sports',
	'Entertainment',
	'Movies',
	'Kids',
	'Music',
	'Religious',
	'Documentary',
	'Shop',
	'Local',
	'Lifestyle',
	'Weather'
];
const US_WORDS = ['Channel', 'TV', 'Network', 'Live', 'Plus', 'One', 'Now', 'Classic', 'Prime', 'Central'];

/** A country of three hundred channels, no guide, over a dozen categories; one in four
 * without a logo, a few blocked or not answering. */
export const STATES: LiveChannel[] = Array.from({ length: 300 }, (_, i) => {
	const cat = US_CATEGORIES[(i * 7) % US_CATEGORIES.length];
	const name = `${cat === 'Local' ? `K${String.fromCharCode(65 + (i % 26))}${String.fromCharCode(65 + ((i * 3) % 26))}` : cat} ${US_WORDS[i % US_WORDS.length]} ${i + 1}`;
	return channel({
		id: `us${i}`,
		name,
		categories: i % 13 === 0 ? [] : [cat],
		logo_url: i % 4 === 0 ? null : logo(name.split(' ')[0].slice(0, 7), i % 3 ? '#ffffff' : '#1b1b1b', i % 3 ? '#1b1b1b' : '#ffffff'),
		quality: i % 5 === 0 ? null : 720,
		geo_blocked: i % 17 === 0,
		not_24_7: i % 11 === 0,
		unreachable: i % 29 === 0
	});
});

/** The backend's answers for these, as stubApi routes. */
export function liveRoutes(now: number, extra: Record<string, unknown> = {}) {
	return {
		'/livetv/countries': { default_country: 'fr', countries: COUNTRIES },
		'/livetv/fr/channels': { country: 'fr', channels: FRANCE },
		'/livetv/fr/epg/now': { entries: franceGuide(now), fetched_at: new Date(now).toISOString() },
		'/livetv/us/channels': { country: 'us', channels: STATES },
		'/livetv/us/epg/now': { entries: [], fetched_at: new Date(now).toISOString() },
		'/livetv/ad/channels': { country: 'ad', channels: [] },
		'/livetv/ad/epg/now': { entries: [], fetched_at: new Date(now).toISOString() },
		...extra
	};
}
