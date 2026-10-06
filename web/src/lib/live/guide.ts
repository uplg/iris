// The Live TV listing, framework-free: which countries come first in the picker, how a typed
// name finds one, how a country's channels fall into sections, and what a channel that may
// not play says about itself.

import type { LiveChannel, LiveCountry } from '@iris/api/client';
import { plural } from '@iris/api/format';
import { json, stored } from '#lib/stored.ts';
import { STORAGE } from '#lib/storage.ts';

/** How many picked countries the browser keeps. */
export const RECENT_COUNTRIES = 3;

const codes = (v: unknown): v is string[] => Array.isArray(v) && v.every((c) => typeof c === 'string' && /^[a-z]{2}$/.test(c));

/** The countries last picked in this browser, most recent first. */
export const recentCountries = stored<string[]>(STORAGE.liveCountries, [], json(codes));

/** `code` picked: first of the recent ones, the oldest falling off. */
export function remember(recent: readonly string[], code: string): string[] {
	return [code, ...recent.filter((c) => c !== code)].slice(0, RECENT_COUNTRIES);
}

/** The household's usual countries, the server's default first, then the ones last picked
 * (only those still offered). */
export function usualCountries(defaultCode: string, recent: readonly string[], offered: readonly LiveCountry[]): LiveCountry[] {
	const byCode = new Map(offered.map((c) => [c.code, c]));
	return [...new Set([defaultCode, ...recent])].flatMap((code) => byCode.get(code) ?? []);
}

/** Case and accents folded: « reunion » finds Réunion. */
export const fold = (s: string) => s.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase().trim();

/** The countries a typed text names: a name starting with it first, then one containing it
 * (« kingdom » finds the United Kingdom), or the two-letter code itself. */
export function findCountries(countries: readonly LiveCountry[], text: string): LiveCountry[] {
	const q = fold(text);
	if (!q) return [...countries];
	const starts: LiveCountry[] = [];
	const within: LiveCountry[] = [];
	for (const c of countries) {
		const name = fold(c.name);
		if (name.startsWith(q) || c.code === q) starts.push(c);
		else if (name.includes(q)) within.push(c);
	}
	return [...starts, ...within];
}

/** « 42 channels », or nothing when the server does not know yet. */
export const channelCount = (c: Pick<LiveCountry, 'channel_count'>) =>
	typeof c.channel_count === 'number' ? plural(c.channel_count, 'channel') : null;

export interface ChannelSection {
	/** `tnt`, or `cat:<category>`: the filter's value. */
	key: string;
	title: string;
	channels: LiveChannel[];
}

export const TNT = 'tnt';
const OTHER = 'Other';

/** The French free-to-air channels by their number, then one section per first category
 * (alphabetical, « Other » last). Within a category, a channel that will likely not play
 * goes to the end; the free-to-air ones keep their numbers' order. */
export function channelSections(channels: readonly LiveChannel[]): ChannelSection[] {
	const tnt = channels.filter((c) => typeof c.tnt_number === 'number').toSorted((a, b) => a.tnt_number! - b.tnt_number!);
	const byCategory = new Map<string, LiveChannel[]>();
	for (const c of channels) {
		if (typeof c.tnt_number === 'number') continue;
		const title = c.categories[0] ?? OTHER;
		byCategory.set(title, [...(byCategory.get(title) ?? []), c]);
	}
	const titles = [...byCategory.keys()].toSorted((a, b) => (a === OTHER ? 1 : b === OTHER ? -1 : a.localeCompare(b)));
	const sinking = (c: LiveChannel) => (c.unreachable || c.geo_blocked ? 1 : 0);
	const sections: ChannelSection[] = titles.map((title) => ({
		key: `cat:${title}`,
		title,
		channels: byCategory
			.get(title)!
			.map((c, i) => [c, i] as const)
			.toSorted(([a, i], [b, j]) => sinking(a) - sinking(b) || i - j)
			.map(([c]) => c)
	}));
	return tnt.length ? [{ key: TNT, title: 'Free-to-air (TNT)', channels: tnt }, ...sections] : sections;
}

/** Why a channel may not play, in words; null when nothing is known against it. */
export function channelNotice(c: Pick<LiveChannel, 'unreachable' | 'geo_blocked' | 'not_24_7'>): string | null {
	if (c.unreachable) return 'Not answering right now';
	if (c.geo_blocked) return 'May be blocked in your country';
	if (c.not_24_7) return 'Not on air all day';
	return null;
}

/** Said less loudly: a channel that will likely not play. */
export const dimmed = (c: Pick<LiveChannel, 'unreachable' | 'geo_blocked'>) => !!c.unreachable || c.geo_blocked;
