import { describe, expect, it } from 'vitest';
import { actionFor, nextSpeed, shortcutList } from './shortcuts.ts';
import { positionText, spokenTime, volumeText } from './spoken.ts';

describe('player keys', () => {
	it('follows the YouTube / Netflix conventions', () => {
		expect(actionFor({ key: ' ' })).toEqual({ kind: 'toggle-play' });
		expect(actionFor({ key: 'k' })).toEqual({ kind: 'toggle-play' });
		expect(actionFor({ key: 'K' })).toEqual({ kind: 'toggle-play' });
		expect(actionFor({ key: 'j' })).toEqual({ kind: 'seek-by', seconds: -10 });
		expect(actionFor({ key: 'ArrowRight' })).toEqual({ kind: 'seek-by', seconds: 10 });
		expect(actionFor({ key: 'ArrowUp' })).toEqual({ kind: 'volume-by', delta: 0.1 });
		expect(actionFor({ key: 'm' })).toEqual({ kind: 'toggle-mute' });
		expect(actionFor({ key: 'f' })).toEqual({ kind: 'fullscreen' });
		expect(actionFor({ key: 'c' })).toEqual({ kind: 'tracks' });
		expect(actionFor({ key: 'p' })).toEqual({ kind: 'pip' });
		expect(actionFor({ key: '?' })).toEqual({ kind: 'shortcuts' });
		expect(actionFor({ key: '4' })).toEqual({ kind: 'seek-to-fraction', fraction: 0.4 });
		expect(actionFor({ key: '>' })).toEqual({ kind: 'speed-by', step: 1 });
		expect(actionFor({ key: 'Escape' })).toEqual({ kind: 'escape' });
	});

	it('leaves modified keys and unknown keys to the browser', () => {
		expect(actionFor({ key: 'f', ctrlKey: true })).toBeNull();
		expect(actionFor({ key: 'l', metaKey: true })).toBeNull();
		expect(actionFor({ key: 'x' })).toBeNull();
		expect(actionFor({ key: 't' })).toBeNull();
	});

	it('has no timeline live: no seeking, no speed', () => {
		expect(actionFor({ key: 'j' }, true)).toBeNull();
		expect(actionFor({ key: '5' }, true)).toBeNull();
		expect(actionFor({ key: '<' }, true)).toBeNull();
		expect(actionFor({ key: ' ' }, true)).toEqual({ kind: 'toggle-play' });
		expect(shortcutList(true).some((s) => s.does.includes('seconds'))).toBe(false);
	});

	it('steps through the speeds, clamped', () => {
		expect(nextSpeed(1, 1)).toBe(1.25);
		expect(nextSpeed(2, 1)).toBe(2);
		expect(nextSpeed(0.5, -1)).toBe(0.5);
		expect(nextSpeed(1.1, -1)).toBe(0.75);
	});
});

describe('times in words', () => {
	it('says a position the way a person would', () => {
		expect(spokenTime(1930)).toBe('32 minutes 10 seconds');
		expect(spokenTime(3600)).toBe('1 hour');
		expect(spokenTime(0)).toBe('0 seconds');
		expect(spokenTime(61)).toBe('1 minute 1 second');
		expect(positionText(1930, 3300)).toBe('32 minutes 10 seconds of 55 minutes');
		expect(positionText(5, null)).toBe('5 seconds');
	});
	it('says a volume', () => {
		expect(volumeText(0.4, false)).toBe('40 percent');
		expect(volumeText(0.4, true)).toBe('Muted');
	});
});
