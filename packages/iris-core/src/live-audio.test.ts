import type Hls from 'hls.js';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const inputDispose = vi.fn();
vi.mock('mediabunny', () => ({
	ALL_FORMATS: [],
	AudioSampleSink: vi.fn(),
	Input: class {
		getAudioTracks = async () => [{}];
		dispose = inputDispose;
	}
}));
vi.mock('./decode/libav-audio-decoder', () => ({ ensureLibavAudioDecoderRegistered: () => {} }));
vi.mock('./stream-source', () => ({ irisUrlSource: () => ({}) }));

const { mountLiveAudio } = await import('./live-audio');

const ctxClose = vi.fn(async () => {});
class FakeAudioContext {
	currentTime = 0;
	destination = {};
	createGain = () => ({ connect: () => {}, gain: { value: 1 } });
	resume = async () => {};
	suspend = async () => {};
	close = ctxClose;
}

function fakeVideo() {
	const listeners = new Map<string, Set<unknown>>();
	const video = {
		muted: false,
		volume: 1,
		addEventListener: (type: string, fn: unknown) => {
			if (!listeners.has(type)) listeners.set(type, new Set());
			listeners.get(type)?.add(fn);
		},
		removeEventListener: (type: string, fn: unknown) => listeners.get(type)?.delete(fn)
	};
	const count = () => [...listeners.values()].reduce((n, s) => n + s.size, 0);
	return { video: video as unknown as HTMLVideoElement, count };
}

describe('mountLiveAudio', () => {
	const windowListeners = new Set<string>();
	beforeEach(() => {
		windowListeners.clear();
		inputDispose.mockClear();
		ctxClose.mockClear();
		vi.stubGlobal('AudioContext', FakeAudioContext);
		vi.stubGlobal('window', {
			addEventListener: (type: string) => windowListeners.add(type),
			removeEventListener: (type: string) => windowListeners.delete(type)
		});
	});

	it('tears everything down when the engine goes while it waits for the first PDT', async () => {
		const { video, count } = fakeVideo();
		// a playing stream that has not exposed its PDT yet: the mount parks on it
		const hls = { latestLevelDetails: null, playingDate: null } as unknown as Hls;
		const abort = new AbortController();
		const mounting = mountLiveAudio(video, hls, '/live.m3u8', abort.signal);
		await vi.waitFor(() => expect(count()).toBeGreaterThan(3));
		expect(windowListeners.size).toBe(2);

		abort.abort();
		await mounting;
		expect(ctxClose).toHaveBeenCalledTimes(1);
		expect(inputDispose).toHaveBeenCalledTimes(1);
		expect(windowListeners.size).toBe(0);
		expect(count()).toBe(0);
	});

	it('does not open anything for an engine already gone', async () => {
		const { video, count } = fakeVideo();
		const abort = new AbortController();
		abort.abort();
		await mountLiveAudio(video, {} as Hls, '/live.m3u8', abort.signal);
		expect(ctxClose).not.toHaveBeenCalled();
		expect(windowListeners.size).toBe(0);
		expect(count()).toBe(0);
	});
});
