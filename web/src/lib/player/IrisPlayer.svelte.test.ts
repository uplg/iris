import { afterEach, describe, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { flushSync } from 'svelte';
import { stubApi } from '#lib/test/api.ts';
import { PlaybackChoices } from '#lib/watch/prefs.ts';
import type { EngineMount } from '@iris/core/engine';
import IrisPlayer from './IrisPlayer.svelte';
import { fakeEngine, fakeMount, testManifest } from './testing.ts';

afterEach(() => document.body.replaceChildren());

async function mountPlayer(props: Record<string, unknown> = {}) {
	const box = document.createElement('div');
	box.style.width = '960px';
	box.style.height = '540px';
	document.body.append(box);
	const engine = fakeEngine();
	const { mount, mounts } = fakeMount(engine);
	const onError = vi.fn();
	await render(IrisPlayer, {
		target: box,
		props: {
			tier: 'B',
			src: '/api/torrents/abc/files/0/stream',
			title: 'Severance',
			manifest: testManifest(),
			startPosition: 120,
			onError,
			mountOverride: mount,
			...props
		}
	});
	await expect.element(page.getByRole('button', { name: 'Pause, Space' })).toBeInTheDocument();
	const stage = box.querySelector<HTMLElement>('.stage')!;
	const key = (k: string) => {
		stage.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true }));
		flushSync();
	};
	return { engine, mounts, onError, key, stage };
}

describe('IrisPlayer', () => {
	it('mounts the engine at the resume point, with the preferred audio and the native subtitles', async () => {
		const { mounts } = await mountPlayer({ preferredAudioLang: 'fr' });
		expect(mounts).toHaveLength(1);
		expect(mounts[0]).toMatchObject({ startPosition: 120, audioTrackIndex: 1, streamUrl: '/api/torrents/abc/files/0/stream' });
		expect(mounts[0].nativeSubs.map((s) => s.stream_idx)).toEqual([3, 4, 5]);
	});

	it('remounts at the playhead for an audio switch on a remounting tier, and saves the language for the series', async () => {
		const api = stubApi({ 'PUT /me/playback-preferences': {} });
		const choices = new PlaybackChoices('col-1');
		choices.adopt({ audio_language: 'en', subtitle_language: 'off' });
		const manifest = testManifest();
		const { mounts, engine, key } = await mountPlayer({
			keptFor: 'Kept for the whole series',
			onAudioTrackChange: (i: number) => void choices.audioPicked(manifest, i),
			manifest
		});
		mounts[0].onTimeUpdate?.(842);
		key('c');
		await expect.element(page.getByText('Kept for the whole series')).toBeVisible();
		await page.getByRole('radio', { name: 'French (VF)' }).click();
		await expect.poll(() => mounts.length).toBe(2);
		expect(mounts[1]).toMatchObject({ startPosition: 842, audioTrackIndex: 1 });
		expect(engine.dispose).toHaveBeenCalled();
		await expect.poll(() => api.sent('PUT', '/me/playback-preferences').length).toBe(1);
		expect(api.sent('PUT', '/me/playback-preferences')[0].body).toEqual({
			audio_language: 'fre',
			subtitle_language: 'off',
			collection_id: 'col-1'
		});
	});

	it('switches audio live on Tier F, without a remount', async () => {
		const { mounts, engine, key } = await mountPlayer({ tier: 'F' });
		key('c');
		await page.getByRole('radio', { name: 'French (VF)' }).click();
		expect(engine.setAudioTrack).toHaveBeenCalledWith('1');
		expect(mounts).toHaveLength(1);
	});

	it('turns native subtitles on through the engine', async () => {
		const onActiveSubtitleChange = vi.fn();
		const { engine, key } = await mountPlayer({ initialSubtitleStreamIdx: null, onActiveSubtitleChange });
		expect(engine.setNativeSubtitle).toHaveBeenLastCalledWith(null);
		key('c');
		await page.getByRole('radio', { name: 'French' }).click();
		expect(engine.setNativeSubtitle).toHaveBeenLastCalledWith(4);
		expect(onActiveSubtitleChange).toHaveBeenLastCalledWith(4);
	});

	it('remounts once, silently, on a decode error just after the tab came back', async () => {
		const { mounts, onError } = await mountPlayer();
		document.dispatchEvent(new Event('visibilitychange'));
		mounts[0].onTimeUpdate?.(300);
		mounts[0].onError(new Error('media error 3 (decode)'));
		await expect.poll(() => mounts.length).toBe(2);
		expect(mounts[1].startPosition).toBe(300);
		expect(onError).not.toHaveBeenCalled();
		// a second one is a real failure
		mounts[1].onError(new Error('media error 3 (decode)'));
		expect(onError).toHaveBeenCalledWith('media error 3 (decode)');
	});

	it('no longer listens to a mount it tore down', async () => {
		const onEnded = vi.fn();
		const { mounts, onError, key } = await mountPlayer({ onEnded });
		key('c');
		await page.getByRole('radio', { name: 'French (VF)' }).click();
		await expect.poll(() => mounts.length).toBe(2);
		mounts[0].onError(new Error('media error 4'));
		mounts[0].onEnded?.();
		expect(onError).not.toHaveBeenCalled();
		expect(onEnded).not.toHaveBeenCalled();
	});

	it('reports a failed mount once, though the engine both says it and throws', async () => {
		const onError = vi.fn();
		const failing: EngineMount = async (opts) => {
			const err = new Error('Tier B: MIME not supported');
			opts.onError(err);
			throw err;
		};
		const box = document.createElement('div');
		document.body.append(box);
		await render(IrisPlayer, {
			target: box,
			props: { tier: 'B', src: '/stream', title: 'Severance', manifest: testManifest(), startPosition: 0, onError, mountOverride: failing }
		});
		await expect.poll(() => onError.mock.calls.length).toBe(1);
		await new Promise((r) => requestAnimationFrame(r));
		expect(onError).toHaveBeenCalledTimes(1);
	});

	it('says a waiting engine in words', async () => {
		const { mounts } = await mountPlayer();
		mounts[0].onBusyChange?.(true);
		await expect.element(page.getByRole('status')).toHaveTextContent('Buffering');
		mounts[0].onBusyChange?.(false);
		await expect.element(page.getByRole('status')).toHaveTextContent('');
	});
});
