// Scenario 15: a native text subtitle (SubRip, served as WebVTT, drawn by the browser from a
// `<track>`) toggled off and on through the panel, the controls shown and hidden in between:
// each cue is drawn once (one row of text, one active cue on one showing track). Also while
// the track still loads: the server streams the WebVTT as ffmpeg extracts it, which takes a
// while on a large file, and Gecko restarts an unfinished load on every mode change without
// dropping the cues the first one already parsed.
import { createServer, request, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import type { Page } from '@playwright/test';
import type { ClipKey } from '../harness/catalog.ts';
import { expect, openWatch, play, stage, test, pauseButton } from '../lib/bench.ts';

interface CueState {
	trackEls: number;
	tracks: number;
	showing: number;
	active: { text: string; line: string }[];
	time: number;
}

async function cueState(page: Page): Promise<CueState> {
	return page.evaluate(() => {
		const v = document.querySelector<HTMLVideoElement>('.video-host video')!;
		const tt = v.textTracks;
		const active: { text: string; line: string }[] = [];
		let showing = 0;
		for (let i = 0; i < tt.length; i++) {
			if (tt[i].mode !== 'showing') continue;
			showing++;
			const cues = tt[i].activeCues;
			for (let j = 0; j < (cues?.length ?? 0); j++) {
				const c = cues![j] as VTTCue;
				active.push({ text: c.text, line: String(c.line) });
			}
		}
		return { trackEls: v.querySelectorAll('track').length, tracks: tt.length, showing, active, time: v.currentTime };
	});
}

/** The rows of white text over the lower half of the picture, as bands (top, height) of rows
 * holding cue ink: one line of cue text is one band. */
async function inkBands(page: Page, shot?: string): Promise<{ top: number; height: number; width: number }[]> {
	const box = (await stage(page).boundingBox())!;
	const png = await page.screenshot({
		path: shot,
		clip: { x: box.x + box.width * 0.25, y: box.y + box.height * 0.45, width: box.width * 0.5, height: box.height * 0.55 }
	});
	return page.evaluate(async (b64) => {
		const img = await createImageBitmap(await (await fetch(`data:image/png;base64,${b64}`)).blob());
		const c = new OffscreenCanvas(img.width, img.height);
		const g = c.getContext('2d')!;
		g.drawImage(img, 0, 0);
		const d = g.getImageData(0, 0, img.width, img.height).data;
		const bands: { top: number; height: number; width: number }[] = [];
		let open: { top: number; height: number; left: number; right: number } | null = null;
		let gap = 0;
		for (let y = 0; y < img.height; y++) {
			let n = 0;
			let left = img.width;
			let right = -1;
			for (let x = 0; x < img.width; x++) {
				const i = (y * img.width + x) * 4;
				// white ink on the cue's box: a white pixel with a dimmed one close by (the test
				// pattern's colours are all saturated; Chromium's box is lighter than the others')
				if (d[i] > 225 && d[i + 1] > 225 && d[i + 2] > 225) {
					const j = (y * img.width + Math.max(0, x - 3)) * 4;
					const k = (y * img.width + Math.min(img.width - 1, x + 3)) * 4;
					const dark = (p: number) => Math.max(d[p], d[p + 1], d[p + 2]) < 150;
					if (dark(j) || dark(k)) {
						n++;
						left = Math.min(left, x);
						right = Math.max(right, x);
					}
				}
			}
			if (n >= 3) {
				if (!open) open = { top: y, height: 0, left, right };
				open.height = y - open.top + 1;
				open.left = Math.min(open.left, left);
				open.right = Math.max(open.right, right);
				gap = 0;
			} else if (open && ++gap > 2) {
				if (open.height >= 6 && open.right - open.left >= 60)
					bands.push({ top: open.top, height: open.height, width: open.right - open.left });
				open = null;
			}
		}
		if (open && open.height >= 6 && open.right - open.left >= 60)
			bands.push({ top: open.top, height: open.height, width: open.right - open.left });
		return bands;
	}, png.toString('base64'));
}

async function pick(page: Page, which: 'off' | 'on') {
	await stage(page).hover();
	await page.getByRole('button', { name: 'Audio and subtitles, C' }).click();
	const panel = page.getByRole('dialog', { name: 'Audio and subtitles' });
	const radios = panel.getByRole('group', { name: 'Subtitles' }).getByRole('radio');
	await (which === 'off' ? radios.first() : radios.last()).check();
	await panel.getByRole('button', { name: 'Close' }).click();
}

const cases: [ClipKey, string][] = [
	['srtMkv', 'B'],
	['srtMp4', 'A']
];

type Bands = { top: number; height: number; width: number }[];

for (const [clip, tier] of cases) {
	test(
		`native subtitle toggled off and on, chrome shown and hidden: each cue drawn once (tier ${tier})`,
		{
			tag: ['@chrome', '@firefox', '@webkit']
		},
		async ({ page, state }, info) => {
			await openWatch(page, state, clip, { tier });
			await pick(page, 'on');
			await play(page);
			const samples: { what: string; state: CueState; bands: Bands }[] = [];
			let n = 0;
			const sample = async (what: string, chrome: boolean) => {
				await expect(page.locator('.chrome')).toHaveClass(chrome ? /\bshown\b/ : /^(?!.*\bshown\b)/);
				// the bar's fade
				await page.waitForTimeout(300);
				// well inside a cue (each holds 5.9 s of every 6), not about to end before the capture
				await expect
					.poll(async () => {
						const t = (await cueState(page)).time % 6;
						return t > 0.3 && t < 5.2;
					})
					.toBe(true);
				const s = await cueState(page);
				const bands = await inkBands(page, info.outputPath(`${String(n++).padStart(2, '0')}-${what}.png`));
				samples.push({ what, state: s, bands });
				console.log(`[measure] tier ${tier} ${what}: ${JSON.stringify(s)} bands ${JSON.stringify(bands)}`);
			};
			for (let round = 0; round < 4; round++) {
				await stage(page).hover();
				await sample(`r${round}-chrome`, true);
				await page.mouse.move(0, 0);
				await sample(`r${round}-bare`, false);
				await pick(page, 'off');
				await pick(page, 'on');
				await sample(`r${round}-toggled-chrome`, true);
				await page.mouse.move(0, 0);
				await sample(`r${round}-toggled-bare`, false);
			}
			for (const s of samples) {
				expect(s.state.trackEls, `<track> elements, ${s.what}`).toBe(1);
				expect(s.state.showing, `showing tracks, ${s.what}`).toBe(1);
				expect(s.state.active.length, `active cues, ${s.what}`).toBe(1);
				expect(s.bands.length, `rows of cue text, ${s.what}: ${JSON.stringify(s.bands)}`).toBe(1);
			}
			// lifted above the bar while it shows, back down once it hides (the same cue on screen)
			const bareTop = Math.min(...samples.filter((s) => s.what.endsWith('bare')).map((s) => s.bands[0].top));
			for (const s of samples.filter((x) => x.what.endsWith('chrome'))) {
				expect(s.bands[0].top, `the cue lifted, ${s.what}`).toBeLessThan(bareTop - 40);
			}
			for (const s of samples.filter((x) => x.what.endsWith('bare'))) {
				expect(s.bands[0].top - bareTop, `the cue down, ${s.what}`).toBeLessThanOrEqual(3);
			}
		}
	);
}

/** A same-origin-enough front for the backend (cookies ignore the port) that hands out the
 * first half of every WebVTT body at once and holds the rest until `release()`. */
async function trickleVtt(upstream: string): Promise<{ base: string; release: () => void; close: () => Promise<void> }> {
	const held: (() => void)[] = [];
	let released = false;
	const server: Server = createServer((req, res) => {
		const up = new URL(req.url ?? '/', upstream);
		// identity bodies: the held one is cut in two as text
		const headers = { ...req.headers, host: up.host, 'accept-encoding': 'identity' };
		const fwd = request(up, { method: req.method, headers }, (r) => {
			if (!/\/sub\/\d+\/track\.vtt/.test(up.pathname) || released) {
				res.writeHead(r.statusCode ?? 502, r.headers);
				r.pipe(res);
				return;
			}
			const chunks: Buffer[] = [];
			r.on('data', (c: Buffer) => chunks.push(c));
			r.on('end', () => {
				const body = Buffer.concat(chunks).toString('utf8');
				const cut = body.indexOf('\n\n', body.length / 2) + 2;
				const { 'content-length': _, ...head } = r.headers;
				res.writeHead(r.statusCode ?? 200, head);
				res.write(body.slice(0, cut));
				held.push(() => res.end(body.slice(cut)));
			});
		});
		req.pipe(fwd);
	});
	await new Promise<void>((ok) => server.listen(0, '127.0.0.1', ok));
	return {
		base: `http://127.0.0.1:${(server.address() as AddressInfo).port}`,
		release: () => {
			released = true;
			for (const f of held.splice(0)) f();
		},
		close: () =>
			new Promise((ok) => {
				server.closeAllConnections();
				server.close(() => ok());
			})
	};
}

async function cueCount(page: Page): Promise<{ readyState: number; cues: number | null; active: number }> {
	return page.evaluate(() => {
		const v = document.querySelector<HTMLVideoElement>('.video-host video')!;
		const el = v.querySelector('track')!;
		let active = 0;
		for (let i = 0; i < v.textTracks.length; i++) if (v.textTracks[i].mode === 'showing') active += v.textTracks[i].activeCues?.length ?? 0;
		return { readyState: el.readyState, cues: el.track.cues?.length ?? null, active };
	});
}

for (const [clip, tier] of cases) {
	test(
		`native subtitle toggled off and on while it still loads: each cue once (tier ${tier})`,
		{
			tag: ['@chrome', '@firefox', '@webkit']
		},
		async ({ page, state }, info) => {
			const front = await trickleVtt(state.baseUrl);
			try {
				await page.request.delete(`/api/torrents/${state.infohash[clip]}/files/0/progress`);
				await page.goto(`${front.base}/watch/${state.infohash[clip]}/0?tier=${tier}`);
				await expect(page.getByRole('button', { name: 'Play, Space' }).or(pauseButton(page))).toBeVisible({ timeout: 60_000 });
				await pick(page, 'on');
				await play(page);
				// the first half parsed, the load still open
				await expect.poll(async () => (await cueCount(page)).cues ?? 0).toBeGreaterThan(0);
				const before = await cueCount(page);
				for (let i = 0; i < 2; i++) {
					await pick(page, 'off');
					await pick(page, 'on');
				}
				front.release();
				await expect.poll(async () => (await cueCount(page)).readyState, { message: 'the track loaded' }).toBe(2);
				await expect.poll(async () => (await cueCount(page)).active).toBeGreaterThan(0);
				await page.mouse.move(0, 0);
				await expect(page.locator('.chrome')).not.toHaveClass(/\bshown\b/);
				await page.waitForTimeout(300);
				const after = await cueCount(page);
				const bands = await inkBands(page, info.outputPath('after.png'));
				console.log(
					`[measure] tier ${tier} toggled while loading: before ${JSON.stringify(before)} after ${JSON.stringify(after)} bands ${JSON.stringify(bands)}`
				);
				expect(after.cues, 'cues on the track (the file holds 10)').toBe(10);
				expect(after.active, 'active cues').toBe(1);
				expect(bands.length, `rows of cue text: ${JSON.stringify(bands)}`).toBe(1);
			} finally {
				front.release();
				await front.close();
			}
		}
	);
}
