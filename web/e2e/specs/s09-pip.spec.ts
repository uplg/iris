// Scenario 9: Document picture in picture in Chrome (headed) closes with the player, on "Next
// episode" or a navigation.
import type { Page } from '@playwright/test';
import { CATALOG } from '../harness/catalog.ts';
import { expect, openWatch, play, stage, test } from '../lib/bench.ts';

const pipOpen = (page: Page) =>
	page.evaluate(() => {
		const d = (window as unknown as { documentPictureInPicture?: { window: Window | null } }).documentPictureInPicture;
		return d ? d.window !== null : null;
	});

// a headless browser opens no PiP window
test.use({ headless: false });

test('"Next episode" from the PiP player closes the window', { tag: ['@chrome'] }, async ({ page, state }) => {
	await openWatch(page, state, 'ep1', { resume: CATALOG.ep1.duration - 4 });
	await play(page);
	await stage(page).press('p');
	await expect.poll(() => pipOpen(page), { message: 'the PiP window never opened' }).toBe(true);
	// the chrome lives in the PiP window now, which Playwright doesn't expose as a page
	const clickNext = () =>
		page.evaluate(() => {
			const d = (window as unknown as { documentPictureInPicture: { window: Window | null } }).documentPictureInPicture;
			const b = d.window?.document.querySelector<HTMLButtonElement>('button[aria-label^="Next: "]');
			b?.click();
			return !!b;
		});
	await expect.poll(clickNext, { timeout: 30_000, message: 'no "Next episode" offered near the end' }).toBe(true);
	await page.waitForURL(new RegExp(`/watch/${state.infohash.ep2}/`));
	await expect.poll(() => pipOpen(page), { message: 'the PiP window outlived its player' }).toBe(false);
});

test('a navigation away closes the window', { tag: ['@chrome'] }, async ({ page, state }) => {
	await openWatch(page, state, 'h264Mp4');
	await play(page);
	await stage(page).press('p');
	await expect.poll(() => pipOpen(page)).toBe(true);
	await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Library' }).click();
	await page.waitForURL(/\/library/);
	await expect.poll(() => pipOpen(page), { message: 'the PiP window outlived its player' }).toBe(false);
});
