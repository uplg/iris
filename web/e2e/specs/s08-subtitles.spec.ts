// Scenario 8: ASS (libass) and PGS (libpgs) overlays on tiers B and C, timed on the engine's
// live clock (M8): a cue shows inside its window and not outside it, the karaoke line fills in
// as it plays, and nothing is posted to the subtitle worker while paused.
import type { Page } from '@playwright/test';
import type { ClipKey } from '../harness/catalog.ts';
import { expect, openWatch, pause, play, stage, test, workerPosts } from '../lib/bench.ts';

/** The `<video>`'s playhead (tiers with one), else -1. */
async function now(page: Page): Promise<number> {
	return page.evaluate(() => document.querySelector<HTMLVideoElement>('.video-host video')?.currentTime ?? -1);
}

const shown = async (page: Page) => Number(await page.getByRole('slider', { name: 'Seek' }).getAttribute('aria-valuenow'));

/** Share of near-white (and of red) pixels where the cues sit: bottom centre of the picture. */
async function cueInk(page: Page): Promise<{ white: number; red: number }> {
	const box = (await stage(page).boundingBox())!;
	const png = await page.screenshot({
		clip: { x: box.x + box.width * 0.3, y: box.y + box.height * 0.78, width: box.width * 0.4, height: box.height * 0.13 }
	});
	return page.evaluate(async (b64) => {
		const img = await createImageBitmap(await (await fetch(`data:image/png;base64,${b64}`)).blob());
		const c = new OffscreenCanvas(img.width, img.height);
		const g = c.getContext('2d')!;
		g.drawImage(img, 0, 0);
		const d = g.getImageData(0, 0, img.width, img.height).data;
		let white = 0;
		let red = 0;
		for (let i = 0; i < d.length; i += 4) {
			if (d[i] > 225 && d[i + 1] > 225 && d[i + 2] > 225) white++;
			else if (d[i] > 200 && d[i + 1] < 60 && d[i + 2] < 60) red++;
		}
		const n = d.length / 4;
		return { white: white / n, red: red / n };
	}, png.toString('base64'));
}

/** The cue ink a fraction into an even second (`on`: a cue shows) or an odd one (none does).
 * With a `<video>` the playhead is read directly; a canvas tier only shows whole seconds
 * (rounded, at 4 Hz), so its tick to N says the clock is in [N-0.5, N-0.25]. */
async function sampleAt(page: Page, on: boolean): Promise<number> {
	if ((await now(page)) >= 0) {
		for (;;) {
			const t = await now(page);
			const sec = Math.floor(t);
			const frac = t - sec;
			if ((on ? sec % 2 === 0 : sec % 2 === 1) && frac > 0.25 && frac < 0.6) return (await cueInk(page)).white;
			await page.waitForTimeout(40);
		}
	}
	let last = await shown(page);
	for (;;) {
		await page.waitForTimeout(30);
		const n = await shown(page);
		if (n === last) continue;
		last = n;
		if (on ? n % 2 === 0 : n % 2 === 1) {
			await page.waitForTimeout(700);
			return (await cueInk(page)).white;
		}
	}
}

async function pickSubtitles(page: Page) {
	await stage(page).hover();
	await page.getByRole('button', { name: 'Audio and subtitles, C' }).click();
	const panel = page.getByRole('dialog', { name: 'Audio and subtitles' });
	await panel.getByRole('radio').last().check();
	await panel.getByRole('button', { name: 'Close' }).click();
}

const cases: [string, ClipKey, 'B' | 'C'][] = [
	['ASS', 'ass', 'B'],
	['ASS', 'ass', 'C'],
	['PGS', 'pgs', 'B'],
	['PGS', 'pgs', 'C']
];

for (const [kind, clip, tier] of cases) {
	test(
		`${kind} on tier ${tier}: cues in their windows, silent worker while paused`,
		{ tag: ['@chrome'] },
		async ({ page, state, logs }) => {
			await openWatch(page, state, clip, { tier });
			await pickSubtitles(page);
			await play(page);
			await expect.poll(() => shown(page), { timeout: 30_000 }).toBeGreaterThan(2);
			// the pointer away: the controls hide at once while playing, off the cues
			await page.mouse.move(0, 0);

			const onInk: number[] = [];
			const offInk: number[] = [];
			for (let i = 0; i < 3; i++) {
				onInk.push(await sampleAt(page, true));
				offInk.push(await sampleAt(page, false));
			}
			console.log(`[measure] ${kind} ${tier}: ink on ${onInk.map((x) => x.toFixed(3))} off ${offInk.map((x) => x.toFixed(3))}`);
			expect(Math.min(...onInk), 'a cue missing inside its window').toBeGreaterThan(kind === 'PGS' ? 0.3 : 0.02);
			expect(Math.max(...offInk), 'a cue showing outside its window').toBeLessThan(0.01);

			await pause(page);
			await page.waitForTimeout(500);
			const a = await workerPosts(page);
			await page.waitForTimeout(3000);
			const b = await workerPosts(page);
			const posted = Object.fromEntries(Object.keys(b).map((k) => [k, b[k] - (a[k] ?? 0)]));
			console.log(`[measure] ${kind} ${tier}: worker posts while paused 3 s: ${JSON.stringify(posted)}; totals ${JSON.stringify(b)}`);
			expect(
				Object.values(posted).reduce((x, y) => x + y, 0),
				`posts while paused: ${JSON.stringify(posted)}`
			).toBeLessThanOrEqual(1);
			expect(logs.matching(/libass\] worker error|libpgs\] loadFromUrl failed/)).toEqual([]);
		}
	);
}

test('ASS karaoke fills in syllable by syllable (tier B)', { tag: ['@chrome'] }, async ({ page, state }) => {
	await openWatch(page, state, 'ass', { tier: 'B', resume: 18 });
	await pickSubtitles(page);
	await play(page);
	await page.mouse.move(0, 0);
	const at = async (t: number) => {
		await expect.poll(() => now(page), { timeout: 30_000 }).toBeGreaterThan(t);
		return cueInk(page);
	};
	// \k100 per syllable from 20 s: at 21.5 two are sung (white), two to come (red); at 23.5
	// all four are sung. (No plain cue overlaps either moment.)
	const early = await at(21.5);
	const late = await at(23.5);
	console.log(
		`[measure] karaoke: 21.5 s white ${early.white.toFixed(3)} red ${early.red.toFixed(3)}; 23.5 s white ${late.white.toFixed(3)} red ${late.red.toFixed(3)}`
	);
	expect(early.red, 'the unsung syllables').toBeGreaterThan(0.005);
	expect(late.white).toBeGreaterThan(early.white);
	expect(late.red).toBeLessThan(early.red / 2);
});
