// Scenario 7: resume on tiers A and F starts near the resume point, not from 0 (L3: seek on
// `loadedmetadata`, hls.js `startPosition`). Plus Tier F on a file whose audio has no language.
import { statSync } from 'node:fs';
import { join } from 'node:path';
import { CATALOG } from '../harness/catalog.ts';
import { expect, openWatch, play, shownTime, test, videoState } from '../lib/bench.ts';

const RESUME = 80;

test(
	'tier A: the media reads jump to the resume point at once',
	{ tag: ['@chrome', '@webkit'] },
	async ({ page, state, streamRequests }) => {
		await openWatch(page, state, 'heavy', { tier: 'A', resume: RESUME });
		await play(page);
		await expect.poll(async () => (await videoState(page))?.currentTime ?? 0, { timeout: 30_000 }).toBeGreaterThan(RESUME);
		const v = await videoState(page);
		expect(v?.currentTime).toBeLessThan(RESUME + 10);

		const size = statSync(join(import.meta.dirname, '..', '.media', CATALOG.heavy.file)).size;
		const resumeByte = (RESUME / CATALOG.heavy.duration) * size;
		const reads = streamRequests
			.filter((r) => r.range)
			.map((r) => ({ start: Number(/bytes=(\d+)-/.exec(r.range ?? '')?.[1] ?? 0), at: r.at }));
		const seen = reads.map((r) => `${r.start}@${r.at}ms`).join(', ');
		console.log(`[measure] tier A resume at ${RESUME}s (byte ~${Math.round(resumeByte)}): range starts ${seen}`);
		// the browser reads the head first (the moov, and on a loopback WebKit pulls tens of MB in
		// that moment), then a read lands on the resume point: at once, not after a canplay's worth
		// of the film's start
		const near = reads.find((r) => Math.abs(r.start - resumeByte) / size < 0.1);
		expect(near, `range starts: ${seen}`).toBeDefined();
		expect(near!.at - reads[0].at, `range starts: ${seen}`).toBeLessThan(1500);
	}
);

test('tier F: hls.js starts at the resume point', { tag: ['@chrome', '@webkit'] }, async ({ page, state, streamRequests }) => {
	await openWatch(page, state, 'heavy', { tier: 'F', resume: RESUME });
	await play(page);
	await expect.poll(() => shownTime(page), { timeout: 90_000 }).toBeGreaterThan(RESUME);
	expect(await shownTime(page)).toBeLessThan(RESUME + 10);
	const segments = streamRequests.map((r) => r.url).filter((u) => /\/play\/[^?]*_\d+\.(m4s|ts)/.test(u));
	console.log(`[measure] tier F resume at ${RESUME}s: first segments ${segments.slice(0, 4).map((u) => u.split('/').pop())}`);
	const first = Number(/_(\d+)\.(m4s|ts)/.exec(segments[0] ?? '')?.[1] ?? -1);
	// 6 s segments: the resume point is in segment ~14; anything near the head means hls.js
	// fetched from 0 first
	expect(first, `first segment: ${segments[0]}`).toBeGreaterThan(RESUME / 6 - 3);
});

test('tier F plays a file whose audio track has no language tag', { tag: ['@chrome'] }, async ({ page, state, logs }) => {
	await openWatch(page, state, 'h264Mp4', { tier: 'F' });
	await play(page);
	// fails as soon as the page says why, not after the whole wait
	const outcome = async () => (logs.has(/manifestLoadError/) ? 'manifestLoadError' : (await shownTime(page)) > 2 ? 'playing' : 'waiting');
	await expect.poll(outcome, { timeout: 60_000 }).not.toBe('waiting');
	expect(await outcome(), 'the server HLS of an untagged audio track (see the backend log: packager)').toBe('playing');
});
