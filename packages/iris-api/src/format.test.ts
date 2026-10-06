import { describe, expect, it } from 'vitest';
import {
	ago,
	clock,
	clockTime,
	dayHeading,
	duration,
	episodeCode,
	fileName,
	formatSize,
	isVideo,
	languageLabel,
	percent,
	plural,
	prettySceneName,
	timeLeft,
	until,
	when
} from './format';

describe('format', () => {
	it('clocks positions, unknown as dashes', () => {
		expect(clock(1930)).toBe('32:10');
		expect(clock(3723)).toBe('1:02:03');
		expect(clock(Number.NaN)).toBe('--:--');
		expect(clock(undefined)).toBe('--:--');
	});

	it('says lengths and what is left as people do', () => {
		expect(duration(45)).toBe('45 s');
		expect(duration(55 * 60)).toBe('55 min');
		expect(duration(72 * 60)).toBe('1 h 12 min');
		expect(duration(2 * 3600)).toBe('2 h');
		// rounded on the whole minutes: never "60 min"
		expect(duration(3570)).toBe('1 h');
		expect(duration(7170)).toBe('2 h');
		expect(duration(3629)).toBe('1 h');
		expect(duration(3631)).toBe('1 h 1 min');
		expect(timeLeft(23 * 60)).toBe('23 min left');
	});

	it('counts with the right noun', () => {
		expect(plural(1, 'download')).toBe('1 download');
		expect(plural(3, 'download')).toBe('3 downloads');
		expect(plural(2, 'series', 'series')).toBe('2 series');
		expect(plural(1204, 'seeder')).toBe('1,204 seeders');
	});

	it('writes episode codes the way the cards do', () => {
		expect(episodeCode(2, 4)).toBe('S2:E4');
		expect(episodeCode(2, null)).toBe('Season 2');
		expect(episodeCode(2, 0)).toBe('Season 2');
		expect(episodeCode(null, 19)).toBe('E19');
		expect(episodeCode(null, null)).toBeNull();
	});

	it('names language tags, jargon kept in brackets', () => {
		expect(languageLabel('fr', 'short')).toBe('French (VF)');
		expect(languageLabel('vost')).toBe('Original audio, French subtitles (VOSTFR)');
		expect(languageLabel('xx')).toBeNull();
	});

	it('reads sizes and shares', () => {
		expect(formatSize(12.4 * 1024 ** 3)).toBe('12.4 GB');
		expect(percent(41.6)).toBe('42%');
	});
});

describe('times and names, said one way', () => {
	it('a clock time is en-GB whatever the browser’s locale; a bad date says nothing', () => {
		const at = new Date(2026, 9, 6, 21, 5);
		expect(clockTime(at.toISOString())).toBe('21:05');
		expect(clockTime('not a date')).toBe('');
		expect(when(at.getTime(), at.getTime() + 60_000)).toBe('21:05');
	});

	it('a file’s own name; nothing for no path', () => {
		expect(fileName('Show/Season 1/Show.S01E01.mkv')).toBe('Show.S01E01.mkv');
		expect(fileName(null)).toBeNull();
	});

	it('the video extensions are one list', () => {
		expect(isVideo('a.m2ts')).toBe(true);
		expect(isVideo('a.srt')).toBe(false);
		expect(prettySceneName('Mercato.2025.FRENCH.1080p.WEB.H265-BOUBA.m2ts')).not.toContain('m2ts');
	});
});

describe('moments in a list', () => {
	const now = new Date(2026, 9, 6, 15, 0).getTime();
	const at = (d: number, h: number, m = 0) => new Date(2026, 9, d, h, m).getTime();

	it('a recent moment to the minute, then by its day', () => {
		expect(ago(now - 20_000, now)).toBe('just now');
		expect(ago(now - 12 * 60_000, now)).toBe('12 min ago');
		expect(ago(at(6, 9, 5), now)).toBe('today at 09:05');
		expect(ago(at(5, 21, 4), now)).toBe('yesterday at 21:04');
		expect(ago(at(1, 12), now)).toBe('on Thursday');
	});

	it('a day heading: today, yesterday, else the whole date', () => {
		expect(dayHeading(at(6, 9), now)).toBe('Today');
		expect(dayHeading(at(5, 23), now)).toBe('Yesterday');
		expect(dayHeading(at(3, 12), now)).toBe('Saturday 3 October');
		expect(dayHeading(new Date(2025, 11, 31, 12).getTime(), now)).toBe('Wednesday 31 December 2025');
	});

	it('how long until a moment', () => {
		expect(until(now + 20 * 60_000, now)).toBe('in 20 min');
		expect(until(now + 5 * 3600_000, now)).toBe('in 5 h');
		expect(until(at(7, 10), now)).toBe('tomorrow at 10:00');
		expect(until(at(9, 15), now)).toBe('in 3 days');
		expect(until(now - 1, now)).toBe('now');
	});
});
