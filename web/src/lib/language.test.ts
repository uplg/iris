import { describe, expect, it } from 'vitest';
import { audioWords, languagesPhrase, subtitleWords } from './language.ts';

describe('playback languages, said one way', () => {
	it('the off choice is « Subtitles off »; no choice is the file’s own', () => {
		expect(subtitleWords('off')).toBe('Subtitles off');
		expect(subtitleWords('fre')).toBe('French');
		expect(subtitleWords(null)).toBe('The file’s own');
		expect(audioWords(null)).toBe('The file’s own');
	});

	it('one phrase for the home, the account and a series', () => {
		expect(languagesPhrase({ audio_language: 'fr', subtitle_language: 'off' })).toBe('audio in French, subtitles off');
		expect(languagesPhrase({ subtitle_language: 'en' })).toBe('subtitles in English');
		expect(languagesPhrase({})).toBeNull();
		expect(languagesPhrase({ audio_language: 'fr', subtitle_language: null }, true)).toBe('audio in French, your usual subtitles');
	});
});
