// Scenario 2: Chrome, tier B (mediabunny → MSE), on the heavy clip so a seek leaves the buffer. Overlapping seek restarts (H3/H4): a held
// arrow key or a burst of scrubber clicks never leaves playback paused, never jumps back to an
// earlier target, never trips the frozen-feed watchdog.
import type { Page } from '@playwright/test';
import { expect, openWatch, play, seekSlider, stage, test, videoState } from '../lib/bench.ts';

/** A seedbox across the internet, not a loopback: every byte range answers 150 ms late, which
 * keeps one restart's awaits open while the next seek arrives. */
async function slowStream(page: Page) {
	await page.route(/\/files\/\d+\/stream/, async (route) => {
		await new Promise((r) => setTimeout(r, 150));
		await route.continue();
	});
}

/** Samples the element for `ms`: the lowest time seen once `floor` was passed, and whether it
 * kept playing. */
async function watch(page: Page, ms: number, floor: number) {
	const samples: { t: number; paused: boolean }[] = [];
	const end = Date.now() + ms;
	while (Date.now() < end) {
		const v = await videoState(page);
		if (v) samples.push({ t: v.currentTime, paused: v.paused });
		await page.waitForTimeout(200);
	}
	const after = samples.filter((s, i) => samples.slice(0, i + 1).some((x) => x.t >= floor));
	return { samples, minAfter: Math.min(...after.map((s) => s.t)), last: samples.at(-1)! };
}

test('a held arrow key: playing, forward, no frozen-feed restart', { tag: ['@chrome'] }, async ({ page, state, logs }) => {
	await slowStream(page);
	await openWatch(page, state, 'heavy', { tier: 'B' });
	await play(page);
	await expect.poll(async () => (await videoState(page))?.currentTime ?? 0).toBeGreaterThan(3);
	const from = (await videoState(page))!.currentTime;
	// key auto-repeat, as fast as the keyboard sends them
	for (let i = 0; i < 5; i++) await stage(page).press('ArrowRight', { delay: 0 });
	const target = from + 50;
	const { minAfter, last, samples } = await watch(page, 8000, target - 2);
	console.log(`[measure] held key from ${from.toFixed(1)}s: last ${last.t.toFixed(1)}s, lowest after arrival ${minAfter.toFixed(1)}s`);
	expect(last.paused, 'playback left paused').toBe(false);
	expect(last.t).toBeGreaterThan(target - 2);
	expect(minAfter, `went back after reaching the target: ${samples.map((s) => s.t.toFixed(1)).join(' ')}`).toBeGreaterThan(target - 3);
	expect(logs.matching(/frozen/i), 'frozen-feed restart').toEqual([]);
	expect(logs.has(/tier B → /)).toBe(false);
});

test('a burst of scrubber clicks lands on the last one', { tag: ['@chrome'] }, async ({ page, state, logs }) => {
	await slowStream(page);
	await openWatch(page, state, 'heavy', { tier: 'B' });
	await play(page);
	await expect.poll(async () => (await videoState(page))?.currentTime ?? 0).toBeGreaterThan(2);
	const box = (await seekSlider(page).boundingBox())!;
	// every target out of what is buffered from the start: each one restarts the pipeline
	for (const f of [0.9, 0.5, 0.95, 0.75]) {
		await stage(page).hover();
		await page.mouse.click(box.x + box.width * f, box.y + box.height / 2);
	}
	const target = 120 * 0.75;
	const { minAfter, last } = await watch(page, 8000, target - 1);
	console.log(`[measure] scrub burst: last ${last.t.toFixed(1)}s (target ${target}s), lowest after arrival ${minAfter.toFixed(1)}s`);
	expect(last.paused).toBe(false);
	expect(last.t).toBeGreaterThan(target);
	expect(last.t).toBeLessThan(target + 12);
	expect(minAfter).toBeGreaterThan(target - 3);
	expect(logs.matching(/frozen/i)).toEqual([]);
	const v = await videoState(page);
	expect(v?.error).toBeNull();
});
