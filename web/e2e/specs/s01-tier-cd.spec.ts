// Scenario 1: Chrome, tiers C and D (WebCodecs + canvas + Web Audio). Paced decode (H1): plays
// at 1x with a sane main-thread load and ends at the real end; pause freezes everything and
// keeps the mute (H2); repeated relative seeks never go back to 0 (M5); 5.1 keeps the centre
// and mono reaches both ears, at the source's 48 kHz (M7); a file with no audio plays (M6).
import type { Page, Request } from '@playwright/test';
import { CATALOG } from '../harness/catalog.ts';
import {
	expect,
	measureRate,
	openWatch,
	pause,
	peakAudio,
	pictureFrozen,
	play,
	seekSlider,
	shownTime,
	stage,
	test,
	waitForTime
} from '../lib/bench.ts';

/** Main-thread busy time over wall time, from CDP (`TaskDuration` is cumulative seconds). */
async function mainThreadLoad(page: Page, ms: number): Promise<number> {
	const cdp = await page.context().newCDPSession(page);
	await cdp.send('Performance.enable');
	const read = async () => {
		const { metrics } = await cdp.send('Performance.getMetrics');
		return metrics.find((m) => m.name === 'TaskDuration')?.value ?? 0;
	};
	const a = await read();
	const t0 = Date.now();
	await page.waitForTimeout(ms);
	const b = await read();
	await cdp.detach();
	return (b - a) / ((Date.now() - t0) / 1000);
}

/** The save the page sends on the engine's `ended` (heartbeats past 90 % say completed too, but
 * carry `playing`). */
function endedSave(r: Request): boolean {
	if (!r.url().endsWith('/progress') || !['PUT', 'POST'].includes(r.method())) return false;
	try {
		const b = JSON.parse(r.postData() ?? '{}') as { completed?: boolean; playing?: boolean };
		return b.completed === true && b.playing === undefined;
	} catch {
		return false;
	}
}

for (const tier of ['C', 'D'] as const) {
	test.describe(`tier ${tier}`, () => {
		test('plays at 1x on a paced decoder, ends at the real end', { tag: ['@chrome'] }, async ({ page, state, logs }, info) => {
			let endedAt: number | null = null;
			page.on('request', (r) => {
				if (endedAt === null && endedSave(r)) endedAt = Date.now();
			});
			await openWatch(page, state, 'h264Mp4', { tier });
			await play(page);
			await waitForTime(page, 2);
			const load = await mainThreadLoad(page, 8000);
			const { rate } = await measureRate(page, 6000);
			console.log(`[measure] tier ${tier}: main-thread load ${(load * 100).toFixed(1)}%, playback rate ${rate.toFixed(2)}x`);
			info.annotations.push({ type: 'measure', description: `main thread ${(load * 100).toFixed(1)}%, rate ${rate.toFixed(2)}x` });
			expect(rate).toBeGreaterThan(0.8);
			expect(rate).toBeLessThan(1.2);
			expect(load, 'an unpaced decode keeps the main thread busy').toBeLessThan(0.5);
			expect(endedAt, 'ended fired mid-film').toBeNull();
			expect(await pictureFrozen(page, 1000), 'the picture must move while playing').toBe(false);

			// from 8 s before the end: the end comes after them, not at once
			const duration = CATALOG.h264Mp4.duration;
			await stage(page).press(String(Math.floor(((duration - 8) / duration) * 10)));
			const seekAt = Date.now();
			const from = await shownTime(page);
			await expect.poll(() => endedAt, { timeout: 30_000, message: 'ended never fired' }).not.toBeNull();
			const took = (endedAt! - seekAt) / 1000;
			console.log(`[measure] tier ${tier}: ended ${took.toFixed(1)}s after a seek to ${from}s of ${duration}s`);
			expect(took).toBeGreaterThan(duration - from - 4);
			expect(logs.has(/tier [CD] → /)).toBe(false);
		});

		test('pause freezes picture and clock, mute survives play', { tag: ['@chrome'] }, async ({ page, state }) => {
			await openWatch(page, state, 'h264Mp4', { tier });
			await play(page);
			await waitForTime(page, 3);
			await stage(page).press('m');
			await expect(page.getByRole('button', { name: 'Unmute, M' })).toBeVisible();
			await pause(page);
			const at = await shownTime(page);
			expect(await pictureFrozen(page, 2000), 'the picture moved while paused').toBe(true);
			expect(await shownTime(page)).toBe(at);
			const ctxState = await page.evaluate(() =>
				(window as unknown as { __benchAudio: () => { state: string }[] }).__benchAudio().map((t) => t.state)
			);
			expect(ctxState).toContain('suspended');
			await play(page);
			await waitForTime(page, at + 2);
			await expect(page.getByRole('button', { name: 'Unmute, M' })).toBeVisible();
		});

		test('repeated relative seeks never land near 0', { tag: ['@chrome'] }, async ({ page, state }) => {
			await openWatch(page, state, 'h264Mp4', { tier });
			await play(page);
			await waitForTime(page, 4);
			const start = await shownTime(page);
			const seen: number[] = [];
			for (let i = 0; i < 3; i++) {
				await stage(page).press('ArrowRight');
				seen.push(await shownTime(page));
				await page.waitForTimeout(120);
			}
			await page.waitForTimeout(2500);
			const end = await shownTime(page);
			seen.push(end);
			console.log(`[measure] tier ${tier}: seeks from ${start}s → ${seen.join(', ')}`);
			expect(Math.min(...seen), `playhead during the seeks: ${seen.join(', ')}`).toBeGreaterThanOrEqual(start);
			expect(end).toBeGreaterThanOrEqual(start + 28);
			expect(end).toBeLessThanOrEqual(start + 36);
			await expect(seekSlider(page)).toBeVisible();
		});
	});
}

test('tier C: 5.1 keeps the centre channel in both ears, at 48 kHz', { tag: ['@chrome'] }, async ({ page, state }) => {
	await openWatch(page, state, 'surround', { tier: 'C' });
	await play(page);
	await waitForTime(page, 2);
	const peak = await peakAudio(page, 3000);
	console.log(`[measure] 5.1 → stereo: L ${peak.left.toFixed(3)} R ${peak.right.toFixed(3)} @ ${peak.sampleRate} Hz`);
	expect(peak.sampleRate).toBe(48_000);
	expect(peak.left, 'centre (dialogue) missing on the left').toBeGreaterThan(0.05);
	expect(peak.right, 'centre (dialogue) missing on the right').toBeGreaterThan(0.05);
});

for (const tier of ['B', 'C'] as const) {
	test(`tier ${tier}: E-AC-3 decodes through libav.js, with sound`, { tag: ['@chrome'] }, async ({ page, state, logs }) => {
		test.skip(!state.libavIris, 'the Iris libav.js variant is not built here (Dockerfile libav-builder stage; IRIS_E2E_LIBAV_DIR)');
		await openWatch(page, state, 'h264Eac3', { tier });
		await play(page);
		await waitForTime(page, 3);
		const rate = await measureRate(page, 10_000);
		console.log(`[measure] E-AC-3 tier ${tier}: ${rate.rate.toFixed(2)}x`);
		expect(logs.has(/libav decode init: codec=eac3/), 'E-AC-3 did not go through libav.js').toBe(true);
		expect(logs.matching(/libav\.js worker unavailable|variant not found/)).toEqual([]);
		expect(rate.rate).toBeGreaterThan(0.85);
		if (tier === 'C') {
			// Web Audio: the bench's tap hears it
			const peak = await peakAudio(page, 3000);
			console.log(`[measure] E-AC-3 tier C: L ${peak.left.toFixed(3)} R ${peak.right.toFixed(3)} @ ${peak.sampleRate} Hz`);
			expect(peak.left).toBeGreaterThan(0.05);
			expect(peak.right).toBeGreaterThan(0.05);
		} else {
			const bytes = await page.evaluate(
				() =>
					(document.querySelector('.video-host video') as HTMLVideoElement & { webkitAudioDecodedByteCount: number })
						.webkitAudioDecodedByteCount
			);
			expect(bytes, 'no audio decoded by the element').toBeGreaterThan(0);
		}
	});
}

test('tier C: mono reaches both ears', { tag: ['@chrome'] }, async ({ page, state }) => {
	await openWatch(page, state, 'mono', { tier: 'C' });
	await play(page);
	await waitForTime(page, 2);
	const peak = await peakAudio(page, 3000);
	console.log(`[measure] mono: L ${peak.left.toFixed(3)} R ${peak.right.toFixed(3)} @ ${peak.sampleRate} Hz`);
	expect(peak.sampleRate).toBe(48_000);
	expect(peak.left).toBeGreaterThan(0.05);
	expect(peak.right).toBeGreaterThan(0.05);
	expect(Math.abs(peak.left - peak.right) / Math.max(peak.left, peak.right)).toBeLessThan(0.2);
});

test('tier C: a file without audio plays on the wall clock', { tag: ['@chrome'] }, async ({ page, state }) => {
	await openWatch(page, state, 'noAudio', { tier: 'C' });
	await play(page);
	await waitForTime(page, 2, 15_000);
	const { rate } = await measureRate(page, 5000);
	expect(rate).toBeGreaterThan(0.8);
	expect(rate).toBeLessThan(1.2);
	expect(await pictureFrozen(page, 1000)).toBe(false);
});
