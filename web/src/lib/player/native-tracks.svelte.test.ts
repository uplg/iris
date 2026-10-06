import { afterEach, describe, expect, it } from 'vitest';
import { appendNativeTrack, videoBackedHandle, type NativeSubtitleTrack } from '@iris/core/engine';

afterEach(() => document.body.replaceChildren());

function setup(vttUrl: string) {
	const video = document.createElement('video');
	document.body.append(video);
	const map = new Map<number, HTMLTrackElement>();
	const sub = { stream_idx: 2, codec: 'subrip', lang: 'eng', title: null, vttUrl } as unknown as NativeSubtitleTrack;
	appendNativeTrack(video, sub, map);
	const handle = videoBackedHandle(video, { dispose: async () => undefined, nativeTrackMap: map });
	return { video, map, handle };
}

const settled = (el: HTMLTrackElement) =>
	new Promise<void>((done) => {
		el.addEventListener('load', () => done(), { once: true });
		el.addEventListener('error', () => done(), { once: true });
	});

describe('native subtitle on/off', () => {
	it('keeps a loaded track: its cues are whole', async () => {
		const url = URL.createObjectURL(new Blob(['WEBVTT\n\n00:00.000 --> 00:05.000\nHello\n'], { type: 'text/vtt' }));
		const { video, map, handle } = setup(url);
		const first = map.get(2)!;
		const loaded = settled(first);
		handle.setNativeSubtitle(2);
		await loaded;
		expect(first.readyState).toBe(HTMLTrackElement.LOADED);
		handle.setNativeSubtitle(null);
		handle.setNativeSubtitle(2);
		expect(map.get(2)).toBe(first);
		expect(first.track.mode).toBe('showing');
		expect(first.track.cues?.length).toBe(1);
		expect(video.querySelectorAll('track')).toHaveLength(1);
	});

	it('turns an unfinished track back on as a fresh element, same source', async () => {
		const { video, map, handle } = setup('/no-such-subtitle.vtt');
		const first = map.get(2)!;
		const failed = settled(first);
		handle.setNativeSubtitle(2);
		await failed;
		expect(first.readyState).toBe(HTMLTrackElement.ERROR);
		handle.setNativeSubtitle(null);
		expect(first.track.mode).toBe('disabled');
		handle.setNativeSubtitle(2);
		const next = map.get(2)!;
		expect(next).not.toBe(first);
		expect(first.isConnected).toBe(false);
		expect(next.getAttribute('src')).toBe('/no-such-subtitle.vtt');
		expect(next.track.mode).toBe('showing');
		expect(video.querySelectorAll('track')).toHaveLength(1);
		expect(video.textTracks).toHaveLength(1);
	});
});
