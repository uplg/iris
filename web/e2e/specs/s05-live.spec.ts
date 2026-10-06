// Scenario 5: Live TV on a local HLS channel (ffmpeg looping a clip into a sliding window, with
// program date-times). A non-tuner source plays on Tier F; the tuner path (tier-c-live) needs
// the tunerd hardware and stays manual.
import { expect, test, videoState } from '../lib/bench.ts';

test('a live channel joins and plays at the live edge on Tier F', { tag: ['@chrome', '@firefox'] }, async ({ page, state, logs }) => {
	test.skip(!state.livePath, 'the local live channel did not come up (see the bench log)');
	await page.goto(state.livePath!);
	await expect(page.getByText('Live', { exact: true })).toBeVisible({ timeout: 60_000 });
	await expect.poll(async () => (await videoState(page))?.currentTime ?? 0, { timeout: 60_000 }).toBeGreaterThan(1);
	const v1 = (await videoState(page))!;
	await page.waitForTimeout(6000);
	const v2 = (await videoState(page))!;
	const rate = (v2.currentTime - v1.currentTime) / 6;
	console.log(`[measure] live F: ${rate.toFixed(2)}x, buffered ${JSON.stringify(v2.buffered)}`);
	expect(v2.paused).toBe(false);
	expect(rate).toBeGreaterThan(0.8);
	expect(v2.error).toBeNull();
	expect(logs.has(/Tier F mount/)).toBe(true);
	expect(logs.matching(/unrecoverable/)).toEqual([]);
});

test('live Tier B (`?tier=B`) is reachable from the web app', { tag: ['@chrome', '@firefox'] }, async ({ page, state, logs }) => {
	test.skip(!state.livePath, 'the local live channel did not come up');
	test.fail(true, 'known gap: LivePlayer ignores ?tier= (liveTier picks C for the tuner, F otherwise)');
	await page.goto(`${state.livePath!}?tier=B`);
	await expect(page.getByText('Live', { exact: true })).toBeVisible({ timeout: 60_000 });
	await expect.poll(() => logs.has(/\[iris-core\] live cycle/), { timeout: 15_000 }).toBe(true);
});
