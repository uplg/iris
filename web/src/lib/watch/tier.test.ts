import { describe, expect, it } from 'vitest';
import type { DecodeTier } from '@iris/core/manifest-client';
import type { TorrentView } from '@iris/api/client';
import { testManifest } from '#lib/player/testing.ts';
import {
	afterDemotions,
	Demotions,
	errorOutcome,
	forcedTier,
	heartbeatDue,
	isNearEnd,
	isWatched,
	nextDemotionTarget,
	notOnDisk,
	playSource,
	playStatusInterval,
	resumeFrom,
	sourceReady,
	subtitleVersion
} from './tier.ts';

const CHROME = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0 Safari/537.36';
const FIREFOX = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 15.6; rv:154.0) Gecko/20100101 Firefox/154.0';
const ANDROID = 'Mozilla/5.0 (Linux; Android 15) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0 Mobile Safari/537.36';
const env = (userAgent: string, hevcNeedsIdrStart = false, demoted: DecodeTier[] = []) => ({
	userAgent,
	hevcNeedsIdrStart,
	demoted: new Set(demoted)
});
const hevc1080 = testManifest();
const hevc4k = testManifest({ video: [{ ...hevc1080.video[0], height: 2160, width: 3840 }] });
const h264 = testManifest({ video: [{ ...hevc1080.video[0], codec: 'h264', codec_string: 'avc1.640028' }] });

describe('the debug override', () => {
	it('pins a tier from ?tier=', () => {
		expect(forcedTier('?tier=f')).toBe('F');
		expect(forcedTier('?x=1&tier=B')).toBe('B');
		expect(forcedTier('?tier=Z')).toBeNull();
		expect(forcedTier('')).toBeNull();
	});
});

describe('demotion', () => {
	it('routes HEVC ≤ 1080p on desktop Chromium from C/D through hevc.js', () => {
		expect(nextDemotionTarget('C', hevc1080, env(CHROME))).toBe('E');
		expect(nextDemotionTarget('D', hevc1080, env(CHROME))).toBe('E');
	});
	it('goes to F past 1080p, on mobile, for H.264, or once E failed', () => {
		expect(nextDemotionTarget('C', hevc4k, env(CHROME))).toBe('F');
		expect(nextDemotionTarget('C', hevc1080, env(ANDROID))).toBe('F');
		expect(nextDemotionTarget('C', h264, env(CHROME))).toBe('F');
		expect(nextDemotionTarget('C', hevc1080, env(CHROME, false, ['E']))).toBe('F');
	});
	it('sends a refused Gecko CRA splice (Tier B) to hevc.js, not to F', () => {
		expect(nextDemotionTarget('B', hevc1080, env(FIREFOX, true))).toBe('E');
		expect(nextDemotionTarget('B', hevc1080, env(FIREFOX, true, ['E']))).toBe('F');
		expect(nextDemotionTarget('B', hevc1080, env(CHROME, false))).toBe('F');
		expect(nextDemotionTarget('B', h264, env(FIREFOX, true))).toBe('F');
	});
	it('demotes a tier once: the dying engine’s later errors are swallowed', () => {
		const d = new Demotions();
		expect(d.demote('B', () => 'F')).toBe('F');
		expect(d.demote('B', () => 'F')).toBeNull();
		expect(afterDemotions('B', d.demoted)).toBe('F');
		expect(afterDemotions('A', d.demoted)).toBe('A');
	});
	it('says the error on A and F (nowhere to go), demotes the others', () => {
		expect(errorOutcome('A')).toBe('say');
		expect(errorOutcome('F')).toBe('say');
		expect(['B', 'C', 'D', 'E'].map((t) => errorOutcome(t as DecodeTier))).toEqual(['demote', 'demote', 'demote', 'demote']);
	});
});

describe('the stream', () => {
	it('is the server HLS for F, the raw bytes otherwise; an outage nonce changes the URL', () => {
		expect(playSource('F', 'abc', 2, 0)).toEqual({
			src: '/api/torrents/abc/files/2/play/master.m3u8',
			type: 'application/vnd.apple.mpegurl'
		});
		expect(playSource('B', 'abc', 2, 0)).toEqual({ src: '/api/torrents/abc/files/2/stream', type: 'video/mp4' });
		expect(playSource('A', 'abc', 2, 3).src).toBe('/api/torrents/abc/files/2/stream?_r=3');
	});
	it('is ready once the manifest is in, or for F once the server cache is', () => {
		expect(sourceReady('B', false, false, true)).toBe(true);
		expect(sourceReady('F', false, true, true)).toBe(false);
		expect(sourceReady('F', true, false, true)).toBe(false);
		expect(sourceReady('F', true, true, true)).toBe(true);
	});
	it('polls play status every second until ready or failed (also while the request fails)', () => {
		expect(playStatusInterval(undefined)).toBe(1000);
		expect(playStatusInterval({ ready: false, reason: 'remuxing' })).toBe(1000);
		expect(playStatusInterval({ ready: true })).toBe(false);
		expect(playStatusInterval({ ready: false, error: 'ffmpeg died' })).toBe(false);
	});
	it('retries the probe and manifest on « not yet on disk » only', () => {
		expect(notOnDisk(new Error('file not yet on disk'))).toBe(true);
		expect(notOnDisk(new Error('no seeders'))).toBe(false);
		expect(notOnDisk(undefined)).toBe(false);
	});
});

describe('progress', () => {
	const t = (o: Partial<TorrentView>) => ({ finished: false, progress_pct: 0, ...o }) as TorrentView;
	it('versions subtitles in 5 % buckets, final once finished', () => {
		expect(subtitleVersion(undefined)).toBe('0');
		expect(subtitleVersion(t({ progress_pct: 42 }))).toBe('8');
		expect(subtitleVersion(t({ progress_pct: 100, finished: true }))).toBe('final');
	});
	it('resumes past 5 s unless finished', () => {
		expect(resumeFrom(null)).toBe(0);
		expect(resumeFrom({ position_seconds: 4, completed: false, last_watched_at: '' })).toBe(0);
		expect(resumeFrom({ position_seconds: 620, completed: false, last_watched_at: '' })).toBe(620);
		expect(resumeFrom({ position_seconds: 620, completed: true, last_watched_at: '' })).toBe(0);
	});
	it('counts watched at 90 %, near the end at 95 %, a heartbeat every 7 s past 5 s', () => {
		expect(isWatched(90, 100)).toBe(true);
		expect(isWatched(89, 100)).toBe(false);
		expect(isWatched(90, null)).toBe(false);
		expect(isNearEnd(95, 100)).toBe(true);
		expect(isNearEnd(94, 100)).toBe(false);
		expect(heartbeatDue(4, 0)).toBe(false);
		expect(resumeFrom({ position_seconds: 5, completed: false, last_watched_at: '' })).toBe(5);
		expect(heartbeatDue(8, 0)).toBe(true);
		expect(heartbeatDue(14, 8)).toBe(false);
		expect(heartbeatDue(15.5, 8)).toBe(true);
		expect(heartbeatDue(300, 2700)).toBe(true);
		expect(heartbeatDue(2695, 2700)).toBe(false);
	});
});
