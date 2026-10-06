// Scenario 13: memory on a phone, in EMULATION (Chrome with the Pixel 7 descriptor: a desktop's
// RAM and decoders, so no OOM killer; the figures are the engine's own footprint). A long
// seek-heavy session per mobile tier: the pick stays in A/B/F, the JS heap stays bounded, and the
// SourceBuffers keep the mobile windows (Tier B: 15 s behind, ~25 s ahead; Tier F: hls.js
// backBufferLength 30 s, maxMaxBufferLength 60 s). IRIS_E2E_LONG=1 doubles the session.
import type { CDPSession, Page } from '@playwright/test';
import { CATALOG } from '../harness/catalog.ts';
import { expect, openWatch, play, seekSlider, stage, test, videoState, type Console } from '../lib/bench.ts';

const SEEKS = process.env.IRIS_E2E_LONG === '1' ? 80 : 40;
const DWELL_MS = 2500;

/** Every SourceBuffer the page creates, so the windows can be read from outside the engine. */
function recordSourceBuffers() {
	const w = window as unknown as { __benchSBs: SourceBuffer[] };
	w.__benchSBs = [];
	const add = MediaSource.prototype.addSourceBuffer;
	MediaSource.prototype.addSourceBuffer = function (this: MediaSource, type: string) {
		const sb = add.call(this, type);
		w.__benchSBs.push(sb);
		return sb;
	};
}

/** The live SourceBuffers' extent around the playhead: seconds kept behind it, held ahead, in all. */
async function windows(page: Page) {
	return page.evaluate(() => {
		const v = document.querySelector<HTMLVideoElement>('.video-host video');
		const t = v?.currentTime ?? 0;
		const out: { behind: number; ahead: number; total: number }[] = [];
		for (const sb of (window as unknown as { __benchSBs: SourceBuffer[] }).__benchSBs) {
			let b: TimeRanges;
			try {
				b = sb.buffered;
			} catch {
				continue; // removed from its MediaSource (a remount)
			}
			let behind = 0;
			let ahead = 0;
			let total = 0;
			for (let i = 0; i < b.length; i++) {
				const s = b.start(i);
				const e = b.end(i);
				total += e - s;
				if (s <= t + 0.5 && e >= t - 0.5) {
					behind = Math.max(0, t - s);
					ahead = Math.max(0, e - t);
				}
			}
			out.push({ behind, ahead, total });
		}
		return out;
	});
}

async function heapMB(cdp: CDPSession): Promise<number> {
	const { metrics } = await cdp.send('Performance.getMetrics');
	return (metrics.find((m) => m.name === 'JSHeapUsedSize')?.value ?? 0) / 1e6;
}

function pickedTier(logs: Console): string | null {
	for (const l of logs.lines) {
		const m = /\[iris-core\] tier ([A-F])\b/.exec(l);
		if (m) return m[1];
	}
	return null;
}

/** Deterministic seek targets over the clip, far apart (each one leaves the buffer). */
function targets(duration: number): number[] {
	let x = 7;
	return Array.from({ length: SEEKS }, () => {
		x = (x * 48271) % 2147483647;
		return 2 + ((x % 1000) / 1000) * (duration - 12);
	});
}

const SESSIONS: { tier: 'A' | 'B' | 'F'; forced: boolean; behindMax: number | null; totalMax: number | null }[] = [
	// A: the browser's own buffer, no SourceBuffer of ours to read
	{ tier: 'A', forced: false, behindMax: null, totalMax: null },
	// B: 15 s behind, trimmed in 5 s steps; ≤ 25 s ahead
	{ tier: 'B', forced: true, behindMax: 15 + 5 + 3, totalMax: 15 + 5 + 25 + 6 },
	// F: backBufferLength 30 s (+ one 6 s segment), forward ≤ maxMaxBufferLength 60 s
	{ tier: 'F', forced: true, behindMax: 30 + 6 + 3, totalMax: 30 + 6 + 60 + 6 }
];

for (const s of SESSIONS) {
	test(
		`tier ${s.tier}: ${SEEKS} seeks, bounded heap and buffer windows (emulation)`,
		{ tag: ['@phone'] },
		async ({ page, state, logs }, info) => {
			test.setTimeout(120_000 + SEEKS * (DWELL_MS + 1500));
			await page.addInitScript(recordSourceBuffers);
			await openWatch(page, state, 'heavy', s.forced ? { tier: s.tier } : {});
			await play(page);
			await expect.poll(async () => (await videoState(page))?.currentTime ?? 0, { timeout: 90_000 }).toBeGreaterThan(2);
			expect(pickedTier(logs), 'a tier the mobile policy bans').toMatch(/^[ABF]$/);
			expect(pickedTier(logs)).toBe(s.tier);

			const cdp = await page.context().newCDPSession(page);
			await cdp.send('Performance.enable');
			await cdp.send('HeapProfiler.collectGarbage');
			const heap0 = await heapMB(cdp);
			const heaps: number[] = [];
			let worstBehind = 0;
			let worstTotal = 0;
			let stuck = 0;
			const duration = CATALOG.heavy.duration;
			for (const target of targets(duration)) {
				// through the chrome's seek bar, as a viewer does (the engines own the seek)
				await stage(page).hover();
				const box = (await seekSlider(page).boundingBox())!;
				await page.mouse.click(box.x + (box.width * target) / duration, box.y + box.height / 2);
				await page.waitForTimeout(DWELL_MS);
				const v = (await videoState(page))!;
				if (v.paused || v.readyState < 3) stuck += 1;
				heaps.push(await heapMB(cdp));
				for (const w of await windows(page)) {
					worstBehind = Math.max(worstBehind, w.behind);
					worstTotal = Math.max(worstTotal, w.total);
				}
			}
			// what stays once the session settles: a forced collection, then the heap
			await page.waitForTimeout(3000);
			await cdp.send('HeapProfiler.collectGarbage');
			const heapEnd = await heapMB(cdp);
			const peak = Math.max(...heaps);
			const summary =
				`tier ${s.tier} (${info.project.name}, emulation): heap ${heap0.toFixed(0)} → peak ${peak.toFixed(0)} → ${heapEnd.toFixed(0)} MB after GC; ` +
				`SourceBuffer worst behind ${worstBehind.toFixed(1)} s, worst total ${worstTotal.toFixed(1)} s; ${stuck}/${SEEKS} samples not playing`;
			console.log(`[measure] ${summary}`);
			await info.attach('memory', {
				body: `${summary}\nheap samples (MB): ${heaps.map((h) => h.toFixed(0)).join(' ')}`,
				contentType: 'text/plain'
			});
			await cdp.detach();

			// no growth across the session: what a forced GC leaves is near the start
			expect(heapEnd, 'the heap grew across the session').toBeLessThan(heap0 + 30);
			expect(peak, 'heap peak').toBeLessThan(heap0 + 120);
			if (s.behindMax !== null) expect(worstBehind, 'back buffer past the mobile window').toBeLessThan(s.behindMax);
			if (s.totalMax !== null) expect(worstTotal, 'SourceBuffer extent past the mobile windows').toBeLessThan(s.totalMax);
			expect(stuck, 'playback did not resume after the seeks').toBeLessThan(SEEKS * 0.25);
		}
	);
}
