// Scenario 14: A/V sync per tier and browser, on the sync clip (a flash and a beep onset on every
// whole second; lib/avsync.ts says how each side is timed). |offset| stays under 45 ms. The
// opt-in drift run (IRIS_E2E_AVSYNC_LONG=1) plays the 30-minute clip and checks the offset does
// not wander (filter it to the tier and browser wanted: `-g "drift.*tier B" --project firefox`).
import type { Page } from '@playwright/test';
import type { BenchState, ClipKey } from '../harness/catalog.ts';
import { installAvSync, readAvSync, startAvSync, syncStats, type SyncStats } from '../lib/avsync.ts';
import { expect, openWatch, play, test, type Console } from '../lib/bench.ts';

const LIMIT_MS = 45;
/** Beep-to-beep scatter: the picture side is timed at the compositor's frame grain (and a
 * headless Gecko's is coarse), so single pairs spread wider than the offset itself. */
const JITTER_MS = 60;
const LONG = process.env.IRIS_E2E_AVSYNC_LONG === '1';

const TIERS: { tier: string; tags: string[] }[] = [
	{ tier: 'A', tags: ['@chrome', '@firefox', '@webkit'] },
	{ tier: 'B', tags: ['@chrome', '@firefox', '@webkit'] },
	{ tier: 'C', tags: ['@chrome', '@firefox'] },
	{ tier: 'D', tags: ['@chrome'] },
	{ tier: 'E', tags: ['@chrome', '@firefox'] },
	{ tier: 'F', tags: ['@chrome', '@firefox', '@webkit'] }
];

async function measure(
	page: Page,
	state: BenchState,
	logs: Console,
	clip: ClipKey,
	tier: string,
	ms: number
): Promise<SyncStats & { mode: string }> {
	// measured: a MediaElementSource (WebKit's only tap, no captureStream) is silent on an MSE
	// element, and on the native one its clock and rVFC's disagree by a constant ~285 ms
	test.skip(test.info().project.use.browserName === 'webkit', 'WebKit offers no trustworthy audio tap on a media element (see measure)');
	await page.addInitScript(installAvSync);
	await openWatch(page, state, clip, { tier });
	await play(page);
	// past the start-up (holds, the first fragments): the steady state is what is judged
	await page.waitForTimeout(4000);
	const mode = await page.evaluate(startAvSync);
	await page.waitForTimeout(ms);
	const a = await page.evaluate(readAvSync);
	expect(logs.matching(new RegExp(`tier ${tier} → `)), 'the tier was demoted').toEqual([]);
	return { ...syncStats(a), mode };
}

for (const { tier, tags } of TIERS) {
	test(`tier ${tier}: picture and sound within ${LIMIT_MS} ms`, { tag: tags }, async ({ page, state, logs }, info) => {
		const s = await measure(page, state, logs, 'sync', tier, 30_000);
		const line = `A/V ${info.project.name} tier ${tier} (${s.mode}): median ${s.median.toFixed(1)} ms, p90 |${s.p90abs.toFixed(1)}| ms, max |${s.maxAbs.toFixed(1)}| ms over ${s.pairs} beeps`;
		console.log(`[measure] ${line}\n[offsets] ${s.offsets.map((o) => o.toFixed(0)).join(' ')}`);
		await info.attach('avsync', {
			body: `${line}\noffsets (ms, audio − picture): ${s.offsets.map((o) => o.toFixed(1)).join(' ')}`,
			contentType: 'text/plain'
		});
		// a headless Gecko compositor skips the two-frame flash now and then (30 beeps played)
		expect(s.pairs, 'beeps paired with flashes').toBeGreaterThanOrEqual(12);
		expect(Math.abs(s.median), 'median offset').toBeLessThan(LIMIT_MS);
		expect(s.p90abs, 'p90 |offset|').toBeLessThan(JITTER_MS);
	});

	test(`drift, 30 minutes: tier ${tier}`, { tag: tags }, async ({ page, state, logs }, info) => {
		test.skip(!LONG, 'opt-in: IRIS_E2E_AVSYNC_LONG=1');
		test.setTimeout(35 * 60_000);
		const s = await measure(page, state, logs, 'syncLong', tier, 29 * 60_000);
		const line = `A/V drift ${info.project.name} tier ${tier} (${s.mode}): median ${s.median.toFixed(1)} ms, max |${s.maxAbs.toFixed(1)}| ms, drift ${s.driftPerMin.toFixed(2)} ms/min over ${s.pairs} beeps`;
		console.log(`[measure] ${line}`);
		await info.attach('avsync-drift', { body: `${line}\n${s.offsets.map((o) => o.toFixed(1)).join(' ')}`, contentType: 'text/plain' });
		expect(s.pairs).toBeGreaterThan(29 * 60 * 0.9);
		expect(Math.abs(s.median)).toBeLessThan(LIMIT_MS);
		expect(s.p90abs).toBeLessThan(JITTER_MS);
		expect(Math.abs(s.driftPerMin), 'the offset wanders').toBeLessThan(1);
	});
}
