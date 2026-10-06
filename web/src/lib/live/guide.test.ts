import { describe, expect, it } from 'vitest';
import type { LiveChannel } from '@iris/api/client';
import { channelCount, channelNotice, channelSections, findCountries, remember, TNT, usualCountries } from './guide.ts';
import { COUNTRIES, FRANCE } from './fixtures.ts';

const ch = (id: string, over: Partial<LiveChannel> = {}): LiveChannel => ({
	id,
	name: id,
	logo_url: null,
	categories: [],
	quality: null,
	geo_blocked: false,
	not_24_7: false,
	tnt_number: null,
	...over
});

describe('the Live TV guide', () => {
	it('remembers the last few countries, most recent first, each once', () => {
		expect(remember([], 'fr')).toEqual(['fr']);
		expect(remember(['fr', 'us'], 'us')).toEqual(['us', 'fr']);
		expect(remember(['it', 'es', 'de'], 'pt')).toEqual(['pt', 'it', 'es']);
	});

	it('puts the default country first, then the ones picked that are still offered', () => {
		expect(usualCountries('fr', ['us', 'fr', 'zz'], COUNTRIES).map((c) => c.code)).toEqual(['fr', 'us']);
	});

	it('finds a country by the start of its name, then inside it, accents and case aside', () => {
		expect(findCountries(COUNTRIES, 'reu').map((c) => c.name)).toEqual(['Réunion']);
		expect(findCountries(COUNTRIES, 'kingdom').map((c) => c.name)).toEqual(['United Kingdom']);
		expect(findCountries(COUNTRIES, 'uni').map((c) => c.code)).toEqual(['ae', 'gb', 'us', 're', 'tn']);
		expect(
			findCountries(COUNTRIES, 'de')
				.map((c) => c.code)
				.slice(0, 2)
		).toEqual(['de', 'dk']);
	});

	it('says a country’s channels in words, nothing when unknown', () => {
		expect(channelCount({ channel_count: 1 })).toBe('1 channel');
		expect(channelCount({ channel_count: 42 })).toBe('42 channels');
		expect(channelCount({})).toBeNull();
	});

	it('lists free-to-air by number, then categories by name, Other last', () => {
		const sections = channelSections([
			ch('b', { categories: ['News'] }),
			ch('m6', { tnt_number: 6 }),
			ch('x'),
			ch('tf1', { tnt_number: 1 }),
			ch('a', { categories: ['Kids'] })
		]);
		expect(sections.map((s) => s.key)).toEqual([TNT, 'cat:Kids', 'cat:News', 'cat:Other']);
		expect(sections[0].channels.map((c) => c.id)).toEqual(['tf1', 'm6']);
	});

	it('moves a channel that will likely not play to the end of its category', () => {
		const [music] = channelSections([
			ch('dead', { categories: ['Music'], unreachable: true }),
			ch('blocked', { categories: ['Music'], geo_blocked: true }),
			ch('fine', { categories: ['Music'] })
		]);
		expect(music.channels.map((c) => c.id)).toEqual(['fine', 'dead', 'blocked']);
	});

	it('says why a channel may not play', () => {
		expect(channelNotice(ch('a', { unreachable: true, geo_blocked: true }))).toBe('Not answering right now');
		expect(channelNotice(ch('a', { geo_blocked: true }))).toBe('May be blocked in your country');
		expect(channelNotice(ch('a', { not_24_7: true }))).toBe('Not on air all day');
		expect(channelNotice(ch('a'))).toBeNull();
	});

	it('sizes the fixtures like the real catalogue', () => {
		expect(COUNTRIES.length).toBeGreaterThanOrEqual(60);
		expect(FRANCE.length).toBe(40);
	});
});
