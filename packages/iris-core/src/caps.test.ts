import { afterEach, describe, expect, it, vi } from 'vitest';
import { msePlaysAudio } from './caps';

const track = (codec: string, codec_string: string | null, browser_native = false) => ({ codec, codec_string, browser_native });

describe('msePlaysAudio', () => {
	afterEach(() => vi.unstubAllGlobals());

	it('takes the server at its word for the codecs every browser plays', () => {
		expect(msePlaysAudio(track('aac', 'mp4a.40.2', true))).toBe(true);
	});

	it('asks this browser about Dolby: Safari passes it through, Chrome re-encodes', () => {
		const asked: string[] = [];
		vi.stubGlobal('MediaSource', {
			isTypeSupported: (t: string) => {
				asked.push(t);
				return t.includes('ec-3');
			}
		});
		expect(msePlaysAudio(track('eac3', 'ec-3'))).toBe(true);
		expect(msePlaysAudio(track('ac3', 'ac-3'))).toBe(false);
		expect(asked).toEqual(['audio/mp4; codecs="ec-3"', 'audio/mp4; codecs="ac-3"']);
	});

	it('never passes through what MSE cannot be asked about', () => {
		vi.stubGlobal('MediaSource', { isTypeSupported: () => true });
		expect(msePlaysAudio(track('eac3', null))).toBe(false);
		expect(msePlaysAudio(track('dts', 'dtsc'))).toBe(false);
		expect(msePlaysAudio(track('flac', 'flac'))).toBe(false);
	});
});
