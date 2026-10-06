// Scenario 4: tier E (hevc.js WASM transcode), on Firefox and, for the engine logic, on Chrome
// too. Rapid seeks keep moving forward (M4); a mount that fails after the MSE intercept is
// installed gives it back before Tier F plays (M2); the initial buffering hold never shows
// "Play" (L14); playback holds at real time.
import type { Page } from '@playwright/test';
import { expect, openWatch, pauseButton, playButton, stage, test, videoState } from '../lib/bench.ts';

/** hevc.js re-encodes to H.264 with WebCodecs in a worker: probed there with its own config
 * (a real encode, `isConfigSupported` alone says yes on builds that then refuse to configure). */
async function needsH264Encoder(page: Page) {
	await page.goto('/');
	const ok = await page.evaluate(
		() =>
			new Promise<boolean>((resolve) => {
				const src = `
					const enc = new VideoEncoder({ output: () => { postMessage(true); }, error: () => postMessage(false) });
					try {
						enc.configure({ codec: 'avc1.640028', width: 1280, height: 720, bitrate: 2e6, framerate: 24,
							hardwareAcceleration: 'no-preference', latencyMode: 'realtime', avc: { format: 'avc' } });
						const f = new VideoFrame(new Uint8Array(1280 * 720 * 1.5), { format: 'I420', codedWidth: 1280, codedHeight: 720, timestamp: 0 });
						enc.encode(f, { keyFrame: true }); f.close(); enc.flush().catch(() => postMessage(false));
					} catch { postMessage(false); }`;
				const w = new Worker(URL.createObjectURL(new Blob([src], { type: 'text/javascript' })));
				w.onmessage = (e) => resolve(e.data === true);
				w.onerror = () => resolve(false);
			})
	);
	test.skip(!ok, 'no working H.264 VideoEncoder in this browser build (Playwright Firefox ships no OpenH264); manual on Zen');
}

/** The page's MediaSource as it was before any script ran, to tell a leaked intercept. */
function rememberNativeMse() {
	const w = window as unknown as Record<string, unknown>;
	w.__benchNativeAddSourceBuffer = MediaSource.prototype.addSourceBuffer;
	w.__benchNativeMediaSource = window.MediaSource;
}

test(
	'no "Play" during the initial hold, then real-time playback',
	{ tag: ['@firefox', '@chrome'] },
	async ({ page, state, logs }, info) => {
		await needsH264Encoder(page);
		await openWatch(page, state, 'hevcAac', { tier: 'E' });
		await stage(page).hover();
		await playButton(page).click();
		const labels: string[] = [];
		const end = Date.now() + 15_000;
		let started = false;
		while (Date.now() < end) {
			labels.push((await playButton(page).isVisible()) ? 'Play' : (await pauseButton(page).isVisible()) ? 'Pause' : '?');
			const v = await videoState(page);
			if (v && v.currentTime > 1 && !v.paused) started = true;
			if (started && labels.length > 20) break;
			await page.waitForTimeout(100);
		}
		expect(started, 'tier E never started').toBe(true);
		expect(
			labels.filter((l) => l === 'Play'),
			'the chrome said "Play" during the hold'
		).toEqual([]);

		const a = (await videoState(page))!.currentTime;
		const t0 = Date.now();
		await page.waitForTimeout(15_000);
		const b = (await videoState(page))!.currentTime;
		const rate = (b - a) / ((Date.now() - t0) / 1000);
		console.log(`[measure] tier E ${info.project.name}: ${rate.toFixed(2)}x over 15 s`);
		expect(rate).toBeGreaterThan(0.85);
		expect(logs.matching(/tier E → /)).toEqual([]);
	}
);

test('rapid seeks move forward and keep playing', { tag: ['@firefox', '@chrome'] }, async ({ page, state, logs }) => {
	await needsH264Encoder(page);
	await openWatch(page, state, 'hevcAac', { tier: 'E' });
	await stage(page).hover();
	await playButton(page).click();
	await expect.poll(async () => (await videoState(page))?.currentTime ?? 0, { timeout: 60_000 }).toBeGreaterThan(2);
	const from = (await videoState(page))!.currentTime;
	for (let i = 0; i < 3; i++) {
		await stage(page).press('ArrowRight');
		await page.waitForTimeout(150);
	}
	const target = from + 30;
	await expect.poll(async () => (await videoState(page))?.currentTime ?? 0, { timeout: 60_000 }).toBeGreaterThan(target);
	const seen: number[] = [];
	for (let i = 0; i < 20; i++) {
		seen.push((await videoState(page))!.currentTime);
		await page.waitForTimeout(250);
	}
	console.log(`[measure] tier E seeks from ${from.toFixed(1)}s: ${seen.map((t) => t.toFixed(1)).join(' ')}`);
	expect(Math.min(...seen), 'jumped back after reaching the target').toBeGreaterThan(target - 3);
	expect(seen.at(-1)!).toBeGreaterThan(seen[0] + 2);
	expect(logs.matching(/tier E → /)).toEqual([]);
});

test(
	'a mount failing after the intercept: Tier F plays with the native MediaSource',
	{ tag: ['@firefox', '@chrome'] },
	async ({ page, state, logs }) => {
		await page.addInitScript(rememberNativeMse);
		// the engine's byte reads get a body no demuxer accepts; the page's HEAD reachability check
		// (and Tier F, which reads /play/) pass untouched
		await page.route(/\/files\/\d+\/stream/, (route) =>
			route.request().method() === 'GET'
				? route.fulfill({ status: 200, contentType: 'video/mp4', body: 'not a video '.repeat(4096) })
				: route.continue()
		);
		await openWatch(page, state, 'h264Mkv', { tier: 'E' });
		await expect.poll(() => logs.has(/tier E → F/), { timeout: 60_000, message: 'no demotion to F' }).toBe(true);
		await stage(page).hover();
		if (await playButton(page).isVisible()) await playButton(page).click();
		await expect.poll(async () => (await videoState(page))?.currentTime ?? 0, { timeout: 90_000 }).toBeGreaterThan(3);
		const leak = await page.evaluate(() => {
			const w = window as unknown as Record<string, unknown>;
			return {
				addSourceBuffer: MediaSource.prototype.addSourceBuffer === w.__benchNativeAddSourceBuffer,
				mediaSource: window.MediaSource === w.__benchNativeMediaSource
			};
		});
		expect(leak, 'the hevc.js MSE intercept outlived the failed mount').toEqual({ addSourceBuffer: true, mediaSource: true });
	}
);
