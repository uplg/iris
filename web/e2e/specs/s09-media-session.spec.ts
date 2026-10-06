// Scenario 9: the OS media session gets the real position and the real speed at 1.5x (L1).
import { CATALOG } from '../harness/catalog.ts';
import { expect, openWatch, play, sessionPositions, stage, test, videoState } from '../lib/bench.ts';

test('media session: real position, and playbackRate 1.5 at 1.5x', { tag: ['@chrome'] }, async ({ page, state }) => {
	await openWatch(page, state, 'h264Mp4');
	await play(page);
	await expect.poll(async () => (await videoState(page))?.currentTime ?? 0).toBeGreaterThan(2);
	await stage(page).press('>');
	await stage(page).press('>');
	await expect.poll(async () => (await videoState(page))?.playbackRate).toBe(1.5);
	await stage(page).press('ArrowRight');
	await page.waitForTimeout(1000);
	const v = (await videoState(page))!;
	const last = (await sessionPositions(page)).at(-1);
	console.log(`[measure] media session last position ${JSON.stringify(last)}; element at ${v.currentTime.toFixed(1)}s`);
	expect(last?.playbackRate).toBe(1.5);
	expect(last?.duration).toBeCloseTo(CATALOG.h264Mp4.duration, 0);
	expect(Math.abs((last?.position ?? 0) - v.currentTime)).toBeLessThan(2.5);
});
