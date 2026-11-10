// Scenario 12: an iPad (Playwright's WebKit with the iPad descriptor: iPadOS UA, viewport,
// touch; emulation, not the real device). The mobile policy keeps it on A, B or F; HEVC goes to
// Tier B under the `hvc1` spelling WebKit's MSE takes, never to F for want of `hev1`; the
// controls work by touch alone.
import type { Page } from '@playwright/test';
import { CATALOG, type ClipKey } from '../harness/catalog.ts';
import { expect, openWatch, pauseButton, playButton, stage, test, videoState, type Console } from '../lib/bench.ts';

function pickedTier(logs: Console): string | null {
	for (const l of logs.lines) {
		const m = /\[iris-core\] tier ([A-F])\b/.exec(l);
		if (m) return m[1];
	}
	return null;
}

const stillStage = (page: Page) => page.locator('.stage.still');

/** A tap on the hidden controls brings them back (and does nothing else). */
async function revealByTap(page: Page) {
	// faded controls stay in the page, transparent: the stage says `still` meanwhile
	if (await stillStage(page).isVisible()) await stage(page).tap();
	await expect(stillStage(page)).toHaveCount(0);
}

/** Play by touch, on the chrome's button. */
async function tapPlay(page: Page) {
	await revealByTap(page);
	if (await playButton(page).isVisible()) await playButton(page).tap();
	await expect(pauseButton(page)).toBeVisible();
}

async function playsAt1x(page: Page) {
	await expect.poll(async () => (await videoState(page))?.currentTime ?? 0, { timeout: 60_000 }).toBeGreaterThan(2);
	const a = (await videoState(page))!;
	const t0 = Date.now();
	await page.waitForTimeout(6000);
	const b = (await videoState(page))!;
	const rate = (b.currentTime - a.currentTime) / ((Date.now() - t0) / 1000);
	expect(b.error).toBeNull();
	return rate;
}

const PICKS: [ClipKey, string, RegExp][] = [
	['h264Mp4', 'MP4 H.264 + AAC', /^A$/],
	['h264Mkv', 'Matroska H.264 + AAC', /^B$/]
];

for (const [clip, what, expected] of PICKS) {
	test(`${what}: the mobile pick (A, B or F), plays at 1x`, { tag: ['@ipad'] }, async ({ page, state, logs }) => {
		await openWatch(page, state, clip);
		await tapPlay(page);
		const rate = await playsAt1x(page);
		const tier = pickedTier(logs);
		console.log(`[measure] iPad ${CATALOG[clip].file}: tier ${tier}, ${rate.toFixed(2)}x`);
		expect(tier).toMatch(expected);
		expect(rate).toBeGreaterThan(0.85);
	});
}

test('HEVC (hev1 in the file): Tier B under the hvc1 spelling, not F', { tag: ['@ipad'] }, async ({ page, state, logs }) => {
	await page.goto('/');
	const mse = await page.evaluate(() => ({
		hev1: MediaSource.isTypeSupported('video/mp4; codecs="hev1.1.6.L93.B0"'),
		hvc1: MediaSource.isTypeSupported('video/mp4; codecs="hvc1.1.6.L93.B0"')
	}));
	console.log(`[measure] iPad WebKit MSE: hev1=${mse.hev1} hvc1=${mse.hvc1}`);
	test.skip(!mse.hvc1, 'this WebKit build has no HEVC in MSE at all: the routing can only be checked on a real iPad');
	// Safari answers no for every `hev1.*` string (Playwright's WebKit says yes): answer as it does
	await page.addInitScript(() => {
		const native = MediaSource.isTypeSupported.bind(MediaSource);
		MediaSource.isTypeSupported = (type: string) => !/\bhev1\./.test(type) && native(type);
	});
	await openWatch(page, state, 'hevcAac');
	await tapPlay(page);
	const rate = await playsAt1x(page);
	const tier = pickedTier(logs);
	console.log(`[measure] iPad HEVC: tier ${tier}, ${rate.toFixed(2)}x`);
	expect(tier, 'HEVC fell to the server remux').toBe('B');
	expect(rate).toBeGreaterThan(0.85);
});

test(
	'HEVC Main 10 + E-AC-3 5.1 (a WEB MULTi rip): Tier B, Dolby passed through when MSE takes it',
	{ tag: ['@ipad'] },
	async ({ page, state, logs }) => {
		await page.goto('/');
		const ec3 = await page.evaluate(() => MediaSource.isTypeSupported('audio/mp4; codecs="ec-3"'));
		console.log(`[measure] iPad WebKit MSE: ec-3=${ec3}`);
		test.skip(!ec3 && !state.libavIris, 'no E-AC-3 in MSE and no Iris libav.js variant: nothing can play this audio');
		await page.addInitScript(() => {
			const native = MediaSource.isTypeSupported.bind(MediaSource);
			MediaSource.isTypeSupported = (type: string) => !/\bhev1\./.test(type) && native(type);
		});
		await openWatch(page, state, 'hevc10Eac3');
		await tapPlay(page);
		const rate = await playsAt1x(page);
		const tier = pickedTier(logs);
		const reencoded = logs.lines.some((l) => l.includes('AudioEncoder →'));
		console.log(`[measure] iPad HEVC + E-AC-3: tier ${tier}, ${rate.toFixed(2)}x, audio ${reencoded ? 're-encoded' : 'passed through'}`);
		expect(tier).toBe('B');
		expect(reencoded, 'Safari plays E-AC-3 itself: no libav decode + AAC re-encode').toBe(!ec3);
		expect(rate).toBeGreaterThan(0.85);
		// WebKit has no audio tap on an MSE element (s14): the decoder's own byte count is
		// the evidence that the Dolby track is decoded, not carried silently
		const heard = async () => {
			const a = (await videoState(page))!.audioBytes;
			await page.waitForTimeout(2000);
			const b = (await videoState(page))!.audioBytes;
			return a === null || b === null ? null : b - a;
		};
		const decoded = await heard();
		console.log(`[measure] iPad HEVC + E-AC-3: audio decoded over 2 s: ${decoded ?? 'n/a'} bytes`);
		if (decoded !== null) expect(decoded, 'audio bytes decoded while playing').toBeGreaterThan(0);
		// mid-file, where every keyframe is a CRA: a resume lands there
		await revealByTap(page);
		const box = (await page.getByRole('slider', { name: 'Seek' }).boundingBox())!;
		await page.touchscreen.tap(box.x + box.width * 0.6, box.y + box.height / 2);
		await expect.poll(async () => (await videoState(page))?.currentTime ?? 0, { timeout: 30_000 }).toBeGreaterThan(170);
		const from = (await videoState(page))!.currentTime;
		await page.waitForTimeout(6000);
		const after = (await videoState(page))!;
		expect(after.error).toBeNull();
		expect(after.currentTime - from).toBeGreaterThan(4);
		expect(logs.matching(/aborted wedged SourceBuffer op/), 'a healthy append was aborted').toEqual([]);
		// a MULTi rip: the other language, from the panel
		await revealByTap(page);
		await page.getByRole('button', { name: 'Audio and subtitles, C' }).tap();
		const panel = page.getByRole('dialog', { name: 'Audio and subtitles' });
		await panel.getByRole('group', { name: 'Audio' }).getByRole('radio').last().check();
		await panel.getByRole('button', { name: 'Close' }).tap();
		const switched = (await videoState(page))!.currentTime;
		await expect.poll(async () => (await videoState(page))?.currentTime ?? 0, { timeout: 30_000 }).toBeGreaterThan(switched + 3);
		expect((await videoState(page))!.error).toBeNull();
		const decodedAfter = await heard();
		console.log(`[measure] iPad HEVC + E-AC-3: second audio track, decoded over 2 s: ${decodedAfter ?? 'n/a'} bytes`);
		if (decodedAfter !== null) expect(decodedAfter, 'audio bytes decoded on the second track').toBeGreaterThan(0);
		expect(logs.matching(/tier B → /), 'the tier was demoted').toEqual([]);
	}
);

test(
	'touch alone: a tap on hidden controls only shows them; Play, Pause and a seek by tap',
	{ tag: ['@ipad'] },
	async ({ page, state }) => {
		await openWatch(page, state, 'h264Mp4');
		await tapPlay(page);
		await expect.poll(async () => (await videoState(page))?.currentTime ?? 0, { timeout: 30_000 }).toBeGreaterThan(1);
		// the controls fade while playing; the tap that brings them back must not pause
		await expect(stillStage(page)).toBeVisible({ timeout: 10_000 });
		await stage(page).tap();
		await expect(stillStage(page)).toHaveCount(0);
		await page.waitForTimeout(600);
		expect((await videoState(page))!.paused, 'the revealing tap paused playback').toBe(false);
		await revealByTap(page);
		await pauseButton(page).tap();
		await expect.poll(async () => (await videoState(page))?.paused).toBe(true);
		const paused = (await videoState(page))!.currentTime;
		await page.waitForTimeout(1500);
		expect((await videoState(page))!.currentTime).toBeCloseTo(paused, 1);
		// the seek bar, tapped at three quarters (paused: the controls stay)
		const box = (await page.getByRole('slider', { name: 'Seek' }).boundingBox())!;
		await page.touchscreen.tap(box.x + box.width * 0.75, box.y + box.height / 2);
		await expect
			.poll(async () => (await videoState(page))?.currentTime ?? 0, { timeout: 15_000 })
			.toBeGreaterThan(CATALOG.h264Mp4.duration * 0.6);
		await revealByTap(page);
		await playButton(page).tap();
		await expect.poll(async () => (await videoState(page))?.paused).toBe(false);
	}
);
