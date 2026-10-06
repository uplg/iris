// Scenario 3: Firefox on macOS, tier B on HEVC open GOP (one IDR, CRA keyframes after): the
// pick routes it to the CRA splice, a resume and seeks start on a CRA without media error 3; with
// E-AC-3 5.1, the audio plays through libav.js in its worker (M12).
import type { Page } from '@playwright/test';
import type { BenchState, ClipKey } from '../harness/catalog.ts';
import { expect, openWatch, play, stage, test, videoState, type Console } from '../lib/bench.ts';

async function needsHevc(page: Page) {
	const hevc = await page.evaluate(() => MediaSource.isTypeSupported('video/mp4; codecs="hev1.1.6.L93.B0"'));
	test.skip(!hevc, 'this Firefox build has no HEVC in MSE');
}

async function resumeAndSeek(page: Page, state: BenchState, logs: Console, clip: ClipKey) {
	await openWatch(page, state, clip, { resume: 30 });
	await play(page);
	await expect.poll(async () => (await videoState(page))?.currentTime ?? 0, { timeout: 60_000 }).toBeGreaterThan(32);
	expect(logs.has(/Tier B \(CRA splice\)/), 'the pick did not route to the CRA splice').toBe(true);

	await stage(page).press('ArrowLeft');
	await stage(page).press('ArrowLeft');
	const back = (await videoState(page))!.currentTime;
	expect(back).toBeLessThan(28);
	await expect.poll(async () => (await videoState(page))?.currentTime ?? 0, { timeout: 30_000 }).toBeGreaterThan(back + 3);
	const v = (await videoState(page))!;
	expect(v.error, 'media error').toBeNull();
	expect(logs.matching(/media error 3/)).toEqual([]);
	expect(logs.matching(/tier B → /)).toEqual([]);
	expect(v.mozHasAudio, 'no audio reached the element').toBe(true);
}

test('HEVC open GOP: resume and seeks start on CRA keyframes', { tag: ['@firefox'] }, async ({ page, state, logs }) => {
	await needsHevc(page);
	await resumeAndSeek(page, state, logs, 'hevcAac');
});

test('HEVC open GOP + E-AC-3 5.1: the audio goes through libav.js in its worker', { tag: ['@firefox'] }, async ({ page, state, logs }) => {
	test.skip(!state.libavIris, 'the Iris libav.js variant is not built here (Dockerfile libav-builder stage; IRIS_E2E_LIBAV_DIR)');
	await needsHevc(page);
	await resumeAndSeek(page, state, logs, 'hevcEac3');
	expect(logs.has(/libav decode init: codec=eac3/), 'E-AC-3 did not go through libav.js').toBe(true);
	expect(logs.matching(/libav\.js worker unavailable/)).toEqual([]);
});

test('the libav.js variant probe tells a missing build from a served one', { tag: ['@firefox', '@chrome'] }, async ({ page, state }) => {
	const res = await page.request.head('/libavjs/libav-6.10.9.0-iris.wasm.mjs');
	const type = res.headers()['content-type'] ?? '';
	if (state.libavIris) {
		expect(res.status()).toBe(200);
		expect(type).toMatch(/javascript/);
	} else {
		// not the SPA's index.html with a 200, which the probe used to read as "present"
		expect(res.status(), `answered ${type}`).toBe(404);
	}
	for (const path of ['/libass/nope.wasm', '/hevcjs/nope.js', '/libpgs/nope.js', '/_app/immutable/nope.js']) {
		expect((await page.request.get(path)).status(), path).toBe(404);
	}
	expect((await page.request.get('/watch/nope/0')).headers()['content-type']).toMatch(/text\/html/);
});
