import { describe, expect, it } from 'vitest';
import { playbackEnded, withinLead, type EndState } from './pacing';

describe('decode pacing', () => {
	it('decodes only a lead ahead of the clock', () => {
		expect(withinLead(600.4, 600, 0.5)).toBe(true);
		expect(withinLead(601, 600, 0.5)).toBe(false);
		// behind the clock (a keyframe before the seek target) always goes
		expect(withinLead(590, 600, 0.5)).toBe(true);
	});
});

describe('playbackEnded', () => {
	const done: EndState = { videoDone: true, lastVideoTs: 3299.9, hasAudio: true, audioDone: true, audioEndTs: 3300 };

	it('is not the end when the decoder reaches EOF but the clock is mid-film', () => {
		expect(playbackEnded(done, 1200)).toBe(false);
	});

	it('is the end once the clock reaches the audio end', () => {
		expect(playbackEnded(done, 3299.95)).toBe(true);
	});

	it('waits for every track to be decoded', () => {
		expect(playbackEnded({ ...done, audioDone: false }, 3300)).toBe(false);
		expect(playbackEnded({ ...done, videoDone: false }, 3300)).toBe(false);
	});

	it('ends on the last frame without audio', () => {
		const silent: EndState = { videoDone: true, lastVideoTs: 100, hasAudio: false, audioDone: false, audioEndTs: null };
		expect(playbackEnded(silent, 99)).toBe(false);
		expect(playbackEnded(silent, 100)).toBe(true);
	});
});
