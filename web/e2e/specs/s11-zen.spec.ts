// Scenario 11: Zen, the household's Gecko, driven over WebDriver BiDi (lib/zen.ts). Tier E
// (hevc.js) on its own H.264 encoder, or the demotion to F when it has none; tier B's CRA
// splice on HEVC open GOP: a resume and a seek back start on CRA keyframes, no media error 3.
import { installAvSync, readAvSync, startAvSync, syncStats } from '../lib/avsync.ts';
import { probeHevcjsEncoder } from '../lib/probes.ts';
import { expect, test, zenKey, zenPlay, zenUntil, zenVideo, zenWatch, type Zen } from '../lib/zen.ts';

async function h264EncoderWorks(zen: Zen): Promise<boolean> {
	return zen.page.evaluate(probeHevcjsEncoder);
}

async function rate(zen: Zen, ms: number): Promise<number> {
	const a = (await zenVideo(zen))!.currentTime;
	const t0 = Date.now();
	await new Promise((r) => setTimeout(r, ms));
	return ((await zenVideo(zen))!.currentTime - a) / ((Date.now() - t0) / 1000);
}

test('tier E: real-time hevc.js, or a prompt demotion to F without an H.264 encoder', { tag: ['@zen'] }, async ({ zen }) => {
	const encoder = await h264EncoderWorks(zen);
	console.log(`[measure] Zen ${zen.version}: H.264 VideoEncoder ${encoder ? 'works' : 'refuses'}`);
	await zenWatch(zen, 'hevcAac', { tier: 'E' });
	const t0 = Date.now();
	await zenPlay(zen);
	if (encoder) {
		await zenUntil('tier E playing', 60_000, async () => ((await zenVideo(zen))?.currentTime ?? 0) > 2);
		const r = await rate(zen, 15_000);
		console.log(`[measure] Zen tier E: ${r.toFixed(2)}x over 15 s`);
		expect(r).toBeGreaterThan(0.85);
		expect(zen.logs.matching(/tier E → /)).toEqual([]);
		return;
	}
	await zenUntil('the demotion to F', 30_000, async () => zen.logs.has(/tier E → F/));
	console.log(`[measure] Zen tier E → F after ${((Date.now() - t0) / 1000).toFixed(1)}s`);
	// the viewer pressed Play during the hold: Tier F carries that intent
	const resumed = await zenUntil('Tier F resuming on its own', 20_000, async () => ((await zenVideo(zen))?.currentTime ?? 0) > 1).then(
		() => true,
		() => false
	);
	expect(resumed, 'Tier F came back paused').toBe(true);
	const r = await rate(zen, 6000);
	console.log(`[measure] Zen tier F after the demotion: ${r.toFixed(2)}x`);
	expect(r).toBeGreaterThan(0.8);
});

test('tier B CRA splice: resume and seek back start on CRA keyframes', { tag: ['@zen'] }, async ({ zen }) => {
	const hevc = await zen.page.evaluate(() => MediaSource.isTypeSupported('video/mp4; codecs="hev1.1.6.L93.B0"'));
	test.skip(!hevc, `Zen ${zen.version} has no HEVC in MSE`);
	await zenWatch(zen, 'hevcAac', { resume: 30 });
	await zenPlay(zen);
	await zenUntil('playing past the resume point', 60_000, async () => ((await zenVideo(zen))?.currentTime ?? 0) > 32);
	expect(zen.logs.has(/Tier B \(CRA splice\)/), 'the pick did not route to the CRA splice').toBe(true);
	await zenKey(zen, 'ArrowLeft');
	await zenKey(zen, 'ArrowLeft');
	await zenUntil('the seek back', 30_000, async () => ((await zenVideo(zen))?.currentTime ?? 99) < 28);
	const back = (await zenVideo(zen))!.currentTime;
	await zenUntil('playing after the seek back', 30_000, async () => ((await zenVideo(zen))?.currentTime ?? 0) > back + 3);
	const r = await rate(zen, 6000);
	const v = (await zenVideo(zen))!;
	console.log(`[measure] Zen CRA splice: back to ${back.toFixed(1)}s, then ${r.toFixed(2)}x`);
	expect(v.error, 'media error').toBeNull();
	expect(zen.logs.matching(/media error 3/)).toEqual([]);
	expect(zen.logs.matching(/tier B → /)).toEqual([]);
	expect(v.mozHasAudio, 'no audio reached the element').toBe(true);
	expect(r).toBeGreaterThan(0.85);
});

// the s14 measurement (lib/avsync.ts), on Zen's element tiers
for (const tier of ['A', 'B', 'E', 'F']) {
	test(`A/V sync, tier ${tier}: picture and sound within 45 ms`, { tag: ['@zen'] }, async ({ zen }, info) => {
		await zen.page.evaluateOnNewDocument(installAvSync);
		await zenWatch(zen, 'sync', { tier });
		await zenPlay(zen);
		await new Promise((r) => setTimeout(r, 4000));
		const mode = await zen.page.evaluate(startAvSync);
		await new Promise((r) => setTimeout(r, 30_000));
		const s = syncStats(await zen.page.evaluate(readAvSync));
		const line = `A/V zen tier ${tier} (${mode}): median ${s.median.toFixed(1)} ms, p90 |${s.p90abs.toFixed(1)}| ms, max |${s.maxAbs.toFixed(1)}| ms over ${s.pairs} beeps`;
		console.log(`[measure] ${line}\n[offsets] ${s.offsets.map((o) => o.toFixed(0)).join(' ')}`);
		await info.attach('avsync', { body: line, contentType: 'text/plain' });
		expect(zen.logs.matching(new RegExp(`tier ${tier} → `)), 'the tier was demoted').toEqual([]);
		expect(s.pairs).toBeGreaterThanOrEqual(12);
		expect(Math.abs(s.median)).toBeLessThan(45);
		expect(s.p90abs).toBeLessThan(60);
	});
}
