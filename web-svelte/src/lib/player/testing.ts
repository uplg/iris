// Test doubles for the player shell: an EngineHandle that records what the chrome asks of it
// (no media at all), its mount, and a manifest.

import { vi } from 'vitest';
import type { EngineHandle, EngineMount, EngineMountOptions } from '@iris/core/engine';
import type { AudioTrack, Manifest, SubtitleTrack } from '@iris/core/manifest-client';

export interface FakeEngine extends EngineHandle {
	state: { time: number; duration: number | null; paused: boolean; volume: number; muted: boolean; buffered: [number, number][] };
	play: ReturnType<typeof vi.fn<() => Promise<void>>>;
	pause: ReturnType<typeof vi.fn<() => void>>;
	seek: ReturnType<typeof vi.fn<(s: number) => void>>;
	setVolume: ReturnType<typeof vi.fn<(v: number) => void>>;
	setMuted: ReturnType<typeof vi.fn<(m: boolean) => void>>;
	setAudioTrack: ReturnType<typeof vi.fn<(id: string) => void>>;
	setNativeSubtitle: ReturnType<typeof vi.fn<(i: number | null) => void>>;
	dispose: ReturnType<typeof vi.fn<() => Promise<void>>>;
}

export function fakeEngine(init: Partial<FakeEngine['state']> = {}): FakeEngine {
	const state = {
		time: 600,
		duration: 3300,
		paused: false,
		volume: 0.5,
		muted: false,
		buffered: [[0, 900]] as [number, number][],
		...init
	};
	return {
		state,
		dispose: vi.fn(async () => undefined),
		currentTime: () => state.time,
		duration: () => state.duration,
		paused: () => state.paused,
		volume: () => state.volume,
		muted: () => state.muted,
		buffered: () => state.buffered,
		play: vi.fn(async () => {
			state.paused = false;
		}),
		pause: vi.fn(() => {
			state.paused = true;
		}),
		seek: vi.fn((s: number) => {
			state.time = s;
		}),
		setVolume: vi.fn((v: number) => {
			state.volume = v;
		}),
		setMuted: vi.fn((m: boolean) => {
			state.muted = m;
		}),
		audioTracks: () => [],
		setAudioTrack: vi.fn(),
		setNativeSubtitle: vi.fn(),
		videoElement: () => null,
		canvasElement: () => null
	};
}

/** A mount that hands over `engine` and keeps the options it was given. */
export function fakeMount(engine: FakeEngine) {
	const mounts: EngineMountOptions[] = [];
	const mount: EngineMount = async (opts) => {
		mounts.push(opts);
		return engine;
	};
	return { mount, mounts };
}

const audio = (stream_idx: number, lang: string, title: string | null, extra: Partial<AudioTrack> = {}): AudioTrack => ({
	stream_idx,
	lang,
	title,
	codec: 'aac',
	channels: 2,
	default: false,
	forced: false,
	browser_native: true,
	...extra
});
const sub = (stream_idx: number, lang: string, title: string | null, extra: Partial<SubtitleTrack> = {}): SubtitleTrack => ({
	stream_idx,
	lang,
	title,
	codec: 'subrip',
	default: false,
	forced: false,
	extractable: true,
	text_based: true,
	url: `/api/torrents/abc/files/0/sub/${stream_idx}/track.srt`,
	...extra
});

export function testManifest(over: Partial<Manifest> = {}): Manifest {
	return {
		schema_version: 1,
		infohash: 'abc',
		file_idx: 0,
		filename: 'Show.S02E04.1080p.mkv',
		container: 'matroska',
		size_bytes: 1_000_000_000,
		duration_s: 3300,
		download: { bytes_complete: 0, progress: 1, ranges_complete: [] },
		header_byte_range: { start: 0, end: 0 },
		index_at_end: false,
		moov_at_start: null,
		tail_byte_range: null,
		video: [{ stream_idx: 0, codec: 'hevc', codec_string: 'hev1.1.6.L120.90', width: 1920, height: 1080, hdr: 'none' }],
		audio: [audio(1, 'eng', 'Original', { default: true }), audio(2, 'fre', 'VF')],
		subtitles: [sub(3, 'eng', 'SDH'), sub(4, 'fre', null), sub(5, 'eng', 'Signs & Songs', { forced: true })],
		chapters: [{ start_s: 0, end_s: 1800, title: 'Part one' }],
		...over
	};
}
