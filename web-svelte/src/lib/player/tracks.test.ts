import { describe, expect, it } from 'vitest';
import { testManifest } from './testing.ts';
import {
	audioLabels,
	classifySubtitles,
	initialAudioIndex,
	initialSubtitle,
	subtitleLabels,
	subtitleOverlayKind,
	tierRequiresRemountForAudio,
	versioned
} from './tracks.ts';
import type { SubtitleTrack } from '@iris/core/manifest-client';

const m = testManifest();
const track = (codec: string): SubtitleTrack => ({ ...m.subtitles[0], codec });

describe('subtitle renderers', () => {
	it('sends text to the native track, ASS to libass, bitmaps to libpgs', () => {
		expect(subtitleOverlayKind(null)).toBe('none');
		expect(subtitleOverlayKind(track('subrip'))).toBe('native');
		expect(subtitleOverlayKind(track('webvtt'))).toBe('native');
		expect(subtitleOverlayKind(track('ASS'))).toBe('ass');
		expect(subtitleOverlayKind(track('ssa'))).toBe('ass');
		expect(subtitleOverlayKind(track('hdmv_pgs_subtitle'))).toBe('pgs');
		expect(subtitleOverlayKind(track('dvb_subtitle'))).toBe('pgs');
		expect(subtitleOverlayKind(track('dvd_subtitle'))).toBe('pgs');
	});

	it('gives native tracks their .vtt extraction URL', () => {
		const withAss = testManifest({ subtitles: [m.subtitles[0], { ...m.subtitles[1], codec: 'ass' }] });
		const { native, overlay } = classifySubtitles(withAss);
		expect(native.map((t) => t.vttUrl)).toEqual(['/api/torrents/abc/files/0/sub/3/track.vtt']);
		expect(overlay.map((t) => t.stream_idx)).toEqual([4]);
	});

	it('versions a URL only when a token is known', () => {
		expect(versioned('/a.vtt', undefined)).toBe('/a.vtt');
		expect(versioned('/a.vtt', 'final')).toBe('/a.vtt?v=final');
	});

	it('remounts for an audio switch on every tier but F', () => {
		expect(['A', 'B', 'C', 'D', 'E'].every(tierRequiresRemountForAudio)).toBe(true);
		expect(tierRequiresRemountForAudio('F')).toBe(false);
	});
});

describe('the audio a mount starts on', () => {
	it('restores a still-valid saved index first', () => {
		expect(initialAudioIndex(m, 1, 'en')).toBe(1);
		expect(initialAudioIndex(m, 7, null)).toBe(0);
	});
	it('then the preferred language, whatever the tag spelling', () => {
		expect(initialAudioIndex(m, undefined, 'fr')).toBe(1);
		expect(initialAudioIndex(m, undefined, 'fra')).toBe(1);
	});
	it('then the default track', () => {
		const mm = testManifest({
			audio: [
				{ ...m.audio[0], default: false },
				{ ...m.audio[1], default: true }
			]
		});
		expect(initialAudioIndex(mm, undefined, 'de')).toBe(1);
	});
});

describe('the subtitle a mount starts on', () => {
	it('keeps « off » when the person turned them off', () => {
		expect(initialSubtitle(m, null, 'fr')).toBeNull();
	});
	it('restores the saved track', () => {
		expect(initialSubtitle(m, 4, 'en')?.stream_idx).toBe(4);
	});
	it('prefers the plain track over SDH and forced in the preferred language', () => {
		const mm = testManifest({ subtitles: [m.subtitles[0], m.subtitles[2], { ...m.subtitles[1], lang: 'eng', stream_idx: 9 }] });
		expect(initialSubtitle(mm, undefined, 'en')?.stream_idx).toBe(9);
	});
	it('never forces another language: absent means off', () => {
		expect(initialSubtitle(m, undefined, 'de')).toBeNull();
		expect(initialSubtitle(m, undefined, 'off')).toBeNull();
	});
	it('without a preference: the default, else the first native track', () => {
		expect(initialSubtitle(m, undefined, null)?.stream_idx).toBe(3);
		const mm = testManifest({
			subtitles: [
				{ ...m.subtitles[0], codec: 'hdmv_pgs_subtitle' },
				{ ...m.subtitles[1], default: true }
			]
		});
		expect(initialSubtitle(mm, undefined, null)?.stream_idx).toBe(4);
	});
});

describe('track names', () => {
	it('names audio unambiguously', () => {
		expect(audioLabels(m.audio)).toEqual(['English, original', 'French (VF)']);
		const ad = testManifest({ audio: [m.audio[0], { ...m.audio[0], title: 'Audio Description', stream_idx: 7 }] });
		expect(audioLabels(ad.audio)).toEqual(['English, original', 'English, audio description']);
	});
	it('tells two same-named tracks apart', () => {
		const twins = [
			{ ...m.audio[0], title: null, channels: 6 },
			{ ...m.audio[0], title: null, channels: 2 }
		];
		expect(audioLabels(twins)).toEqual(['English, 5.1', 'English, stereo']);
	});
	it('names subtitles by language and purpose', () => {
		expect(subtitleLabels(m.subtitles)).toEqual(['English, for deaf and hard of hearing (SDH)', 'French', 'English, signs and songs only']);
	});
});
