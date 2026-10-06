// The watch page's per-session keepers: progress heartbeats, language choices, the facts line.
import { describe, expect, it, vi } from 'vitest';
import type { TorrentView } from '@iris/api/client';
import { testManifest } from '#lib/player/testing.ts';
import { factsLine, pictureWords } from './facts.ts';
import { keptForText, PlaybackChoices } from './prefs.ts';
import { ProgressSaver } from './progress.ts';

describe('progress', () => {
	const saver = () => {
		const put = vi.fn(async () => undefined);
		const beacon = vi.fn();
		return { s: new ProgressSaver('abc', 2, { put, beacon }), put, beacon };
	};

	it('beats every 7 s of playback, carrying the restored picks and the watched flag', () => {
		const { s, put } = saver();
		s.restore({ position_seconds: 0, completed: false, last_watched_at: '', audio_track_idx: 1, subtitle_track_idx: 4 });
		s.durationChange(100);
		s.timeUpdate(3);
		s.timeUpdate(8);
		s.timeUpdate(12);
		s.timeUpdate(91);
		expect(put.mock.calls.map((c) => (c as unknown[])[2])).toEqual([
			{
				position_seconds: 8,
				duration_seconds: 100,
				audio_track_idx: 1,
				subtitle_track_idx: 4,
				completed: false,
				playing: true,
				seek: false
			},
			{
				position_seconds: 91,
				duration_seconds: 100,
				audio_track_idx: 1,
				subtitle_track_idx: 4,
				completed: true,
				playing: true,
				seek: false
			}
		]);
	});

	it('flags the save after a seek, once', () => {
		const { s, put } = saver();
		s.seekPending = true;
		s.pause(30);
		s.pause(31);
		expect(put.mock.calls.map((c) => ((c as unknown[])[2] as { seek: boolean }).seek)).toEqual([true, false]);
	});

	it('saves the end as completed, and a beacon when the page goes', () => {
		const { s, put, beacon } = saver();
		s.durationChange(100);
		s.timeUpdate(8);
		s.timeUpdate(12);
		s.ended();
		expect((put.mock.calls.at(-1) as unknown[] | undefined)?.[2]).toMatchObject({ position_seconds: 100, completed: true });
		s.flush();
		expect(beacon).toHaveBeenCalledWith('/api/torrents/abc/files/2/progress', expect.stringContaining('"position_seconds":12'));
		beacon.mockClear();
		s.flush();
		expect(beacon).not.toHaveBeenCalled();
	});
});

describe('language choices', () => {
	const m = testManifest();

	it('keeps them for the series when the file belongs to one', async () => {
		const save = vi.fn(async () => undefined);
		const c = new PlaybackChoices('col-1', save);
		c.adopt({ audio_language: 'en', subtitle_language: 'off' });
		await c.audioPicked(m, 1);
		await c.subtitlePicked(m, 3);
		await c.subtitlePicked(m, null);
		expect(save.mock.calls.map((x) => (x as unknown[])[0])).toEqual([
			{ audio_language: 'fre', subtitle_language: 'off', collection_id: 'col-1' },
			{ audio_language: 'fre', subtitle_language: 'eng', collection_id: 'col-1' },
			{ audio_language: 'fre', subtitle_language: 'off', collection_id: 'col-1' }
		]);
		expect(keptForText('col-1')).toBe('Kept for the whole series');
	});

	it('keeps them account-wide otherwise; a track without a tag changes nothing', () => {
		const save = vi.fn(async () => undefined);
		const c = new PlaybackChoices(null, save);
		expect(c.audioPicked(testManifest({ audio: [{ ...m.audio[0], lang: null }] }), 0)).toBeNull();
		void c.audioPicked(m, 0);
		expect(save).toHaveBeenCalledWith({ audio_language: 'eng', subtitle_language: null });
		expect(keptForText(null)).toBe('Kept as your default');
	});
});

describe('the facts line', () => {
	it('says where it plays from, the picture and the path, in words', () => {
		const t = { finished: true, progress_pct: 100 } as TorrentView;
		expect(factsLine(t, testManifest(), 'A')).toBe('Playing from disk · 1080p HEVC · direct');
		expect(factsLine({ finished: false, progress_pct: 41.6 } as TorrentView, testManifest(), 'F')).toBe(
			'Playing while it downloads, 42% · 1080p HEVC · remuxed on the server'
		);
		expect(pictureWords({ ...testManifest().video[0], codec: 'av1', height: 2160, hdr: 'dovi' })).toBe('2160p AV1 Dolby Vision');
		expect(factsLine(undefined, undefined, null)).toBe('');
	});
});
