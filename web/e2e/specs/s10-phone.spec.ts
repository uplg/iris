// Scenario 10: a phone (Pixel 7 emulation in Chrome: viewport, touch, Android UA). The mobile
// policy keeps Matroska on Tier B (mobile buffer window); with E-AC-3, the audio goes through
// libav.js in its worker. Functional only: an emulated phone has a desktop's memory, so the
// reduced cache can't be judged here.
import type { Page } from '@playwright/test';
import type { BenchState, ClipKey } from '../harness/catalog.ts';
import { expect, openWatch, play, test, videoState, type Console } from '../lib/bench.ts';

async function playsOnMobileB(page: Page, state: BenchState, logs: Console, clip: ClipKey) {
	await openWatch(page, state, clip);
	await play(page);
	await expect.poll(async () => (await videoState(page))?.currentTime ?? 0, { timeout: 60_000 }).toBeGreaterThan(2);
	const a = (await videoState(page))!;
	const t0 = Date.now();
	await page.waitForTimeout(8000);
	const b = (await videoState(page))!;
	const rate = (b.currentTime - a.currentTime) / ((Date.now() - t0) / 1000);
	console.log(`[measure] phone tier B (${clip}): ${rate.toFixed(2)}x, audio bytes ${a.audioBytes} → ${b.audioBytes}`);
	expect(logs.has(/\[iris-core\] tier B \{|\[iris-core\] tier B JSHandle/), 'the mobile pick was not B').toBe(true);
	expect(logs.has(/Tier B buffer window: .*mobile=true/)).toBe(true);
	expect(rate).toBeGreaterThan(0.85);
	expect(b.audioBytes ?? 0, 'no audio decoded').toBeGreaterThan(a.audioBytes ?? 0);
	expect(b.error).toBeNull();
}

test('Matroska on a phone: Tier B with the mobile window, plays at 1x with sound', { tag: ['@phone'] }, async ({ page, state, logs }) => {
	await playsOnMobileB(page, state, logs, 'h264Mkv');
});

test('E-AC-3 on a phone: Tier B, audio through the libav.js worker', { tag: ['@phone'] }, async ({ page, state, logs }) => {
	test.skip(!state.libavIris, 'the Iris libav.js variant is not built here (Dockerfile libav-builder stage; IRIS_E2E_LIBAV_DIR)');
	await playsOnMobileB(page, state, logs, 'h264Eac3');
	expect(logs.has(/libav decode init: codec=eac3/)).toBe(true);
	expect(logs.matching(/libav\.js worker unavailable/)).toEqual([]);
});
