// What every spec shares: the bench state, a signed-in page, the console kept for the report,
// in-page probes (audio levels, worker posts, media session calls) and player helpers.
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test as base, expect, type Page } from '@playwright/test';
import { CATALOG, type BenchState, type ClipKey } from '../harness/catalog.ts';

export { expect };

export function readState(): BenchState {
	return JSON.parse(readFileSync(join(import.meta.dirname, '..', '.state.json'), 'utf8')) as BenchState;
}

export interface Console {
	lines: string[];
	has(re: RegExp): boolean;
	matching(re: RegExp): string[];
}

/** Installed before any page script: counters the specs read back with `page.evaluate`. */
function probes() {
	type W = typeof window & Record<string, unknown>;
	const w = window as W;

	// audio: a tap on every node that feeds a destination, upmixed like the speakers would be
	const taps: { left: AnalyserNode; right: AnalyserNode; ctx: BaseAudioContext }[] = [];
	w.__benchTaps = taps;
	const connect = AudioNode.prototype.connect as (this: AudioNode, ...a: unknown[]) => unknown;
	AudioNode.prototype.connect = function (this: AudioNode, ...args: unknown[]) {
		const out = connect.apply(this, args);
		const dest = args[0];
		if (dest instanceof AudioDestinationNode && !(this as unknown as Record<string, unknown>).__benchTap) {
			const ctx = this.context;
			const up = ctx.createGain();
			up.channelCount = 2;
			up.channelCountMode = 'explicit';
			up.channelInterpretation = 'speakers';
			const split = ctx.createChannelSplitter(2);
			const left = ctx.createAnalyser();
			const right = ctx.createAnalyser();
			for (const n of [up, split, left, right]) (n as unknown as Record<string, unknown>).__benchTap = true;
			connect.call(this, up);
			connect.call(up, split);
			connect.call(split, left, 0);
			connect.call(split, right, 1);
			taps.push({ left, right, ctx });
		}
		return out;
	} as typeof AudioNode.prototype.connect;
	w.__benchAudio = () => {
		const rms = (a: AnalyserNode) => {
			const buf = new Float32Array(a.fftSize);
			a.getFloatTimeDomainData(buf);
			let s = 0;
			for (const v of buf) s += v * v;
			return Math.sqrt(s / buf.length);
		};
		return taps.map((t) => ({
			left: rms(t.left),
			right: rms(t.right),
			sampleRate: t.ctx.sampleRate,
			state: (t.ctx as AudioContext).state
		}));
	};

	// workers: posts per script, so a paused overlay can be seen posting (or not)
	const posts: Record<string, number> = {};
	w.__benchPosts = posts;
	const NativeWorker = window.Worker;
	const Wrapped = function (this: unknown, url: string | URL, opts?: WorkerOptions) {
		const worker = new NativeWorker(url, opts);
		const name = String(url).split('/').pop() ?? String(url);
		const post = worker.postMessage.bind(worker) as (...a: unknown[]) => void;
		worker.postMessage = ((...a: unknown[]) => {
			posts[name] = (posts[name] ?? 0) + 1;
			post(...a);
		}) as Worker['postMessage'];
		return worker;
	} as unknown as typeof Worker;
	Wrapped.prototype = NativeWorker.prototype;
	window.Worker = Wrapped;

	// media session: every position pushed to the OS
	const positions: MediaPositionState[] = [];
	w.__benchPositions = positions;
	if ('mediaSession' in navigator && typeof navigator.mediaSession.setPositionState === 'function') {
		const set = navigator.mediaSession.setPositionState.bind(navigator.mediaSession);
		navigator.mediaSession.setPositionState = (s?: MediaPositionState) => {
			if (s) positions.push({ ...s });
			set(s);
		};
	}
}

export interface StreamRequest {
	url: string;
	range: string | null;
	client: string | null;
	at: number;
}

export const test = base.extend<{ state: BenchState; logs: Console; streamRequests: StreamRequest[] }>({
	// oxlint-disable-next-line no-empty-pattern -- Playwright reads the fixture's dependencies from this destructuring
	state: async ({}, use) => {
		await use(readState());
	},
	logs: async ({ page }, use, info) => {
		const lines: string[] = [];
		page.on('console', (m) => lines.push(`[${m.type()}] ${m.text()}`));
		page.on('pageerror', (e) => lines.push(`[pageerror] ${e.message}`));
		await use({ lines, has: (re) => lines.some((l) => re.test(l)), matching: (re) => lines.filter((l) => re.test(l)) });
		writeFileSync(info.outputPath('console.txt'), lines.join('\n'));
		await info.attach('console', { path: info.outputPath('console.txt'), contentType: 'text/plain' });
	},
	streamRequests: async ({ page }, use) => {
		const seen: StreamRequest[] = [];
		const t0 = Date.now();
		page.on('request', (r) => {
			if (!/\/api\/torrents\/[0-9a-f]+\/files\/\d+\/(stream|play\/)/.test(r.url())) return;
			const h = r.headers();
			seen.push({ url: r.url(), range: h.range ?? null, client: h['x-iris-client'] ?? null, at: Date.now() - t0 });
		});
		await use(seen);
	},
	page: async ({ page, state }, use) => {
		await page.addInitScript(probes);
		const res = await page.request.post('/api/auth/login', { data: { email: state.email, password: state.password } });
		expect(res.ok(), `login: ${res.status()}`).toBe(true);
		await use(page);
	}
});

export interface WatchOptions {
	/** `?tier=`: the engine pinned. */
	tier?: string;
	/** A saved position to resume from; without it the clip's progress is forgotten first, so
	 * one spec's playback never becomes another's resume point. */
	resume?: number;
	query?: Record<string, string>;
}

/** Opens a clip's watch page and waits for the player's controls. */
export async function openWatch(page: Page, state: BenchState, clip: ClipKey, opts: WatchOptions = {}) {
	const path = `/api/torrents/${state.infohash[clip]}/files/0/progress`;
	if (opts.resume) {
		const res = await page.request.put(path, {
			data: { position_seconds: opts.resume, duration_seconds: CATALOG[clip].duration, seek: true }
		});
		expect(res.ok(), `saving the resume point: ${res.status()}`).toBe(true);
	} else {
		await page.request.delete(path);
	}
	const q = new URLSearchParams(opts.query);
	if (opts.tier) q.set('tier', opts.tier);
	await page.goto(`/watch/${state.infohash[clip]}/0${q.size ? `?${q}` : ''}`);
	await expect(playButton(page).or(pauseButton(page))).toBeVisible({ timeout: 60_000 });
}

export const playButton = (page: Page) => page.getByRole('button', { name: 'Play, Space' });
export const pauseButton = (page: Page) => page.getByRole('button', { name: 'Pause, Space' });
export const seekSlider = (page: Page) => page.getByRole('slider', { name: 'Seek' });

/** Starts playback the way a user does (the button); an engine that already plays is left be. */
export async function play(page: Page) {
	await stage(page).hover();
	if (await playButton(page).isVisible()) await playButton(page).click();
	await expect(pauseButton(page)).toBeVisible();
}

export async function pause(page: Page) {
	await stage(page).hover();
	await pauseButton(page).click();
	await expect(playButton(page)).toBeVisible();
}

export const stage = (page: Page) => page.locator('.stage').first();

/** The playhead as the chrome shows it (every tier, canvas ones included), whole seconds. */
export async function shownTime(page: Page): Promise<number> {
	return Number(await seekSlider(page).getAttribute('aria-valuenow'));
}

/** The `<video>` element's state, when the engine has one. */
export async function videoState(page: Page) {
	return page.evaluate(() => {
		const v = document.querySelector<HTMLVideoElement>('.video-host video');
		if (!v) return null;
		const ranges: [number, number][] = [];
		for (let i = 0; i < v.buffered.length; i++) ranges.push([v.buffered.start(i), v.buffered.end(i)]);
		const ext = v as HTMLVideoElement & { webkitAudioDecodedByteCount?: number; mozHasAudio?: boolean; webkitDecodedFrameCount?: number };
		return {
			currentTime: v.currentTime,
			paused: v.paused,
			ended: v.ended,
			readyState: v.readyState,
			playbackRate: v.playbackRate,
			duration: v.duration,
			error: v.error ? `${v.error.code} ${v.error.message}` : null,
			buffered: ranges,
			audioBytes: ext.webkitAudioDecodedByteCount ?? null,
			mozHasAudio: ext.mozHasAudio ?? null,
			frames: v.getVideoPlaybackQuality?.().totalVideoFrames ?? ext.webkitDecodedFrameCount ?? null
		};
	});
}

/** Waits until the shown playhead passes `seconds`. */
export async function waitForTime(page: Page, seconds: number, timeout = 30_000) {
	await expect.poll(() => shownTime(page), { timeout, message: `playhead past ${seconds}s` }).toBeGreaterThanOrEqual(seconds);
}

/** Measures the playback rate over `ms` of wall clock, from the chrome's playhead. */
export async function measureRate(page: Page, ms: number): Promise<{ rate: number; from: number; to: number }> {
	const from = await shownTime(page);
	const t0 = Date.now();
	await page.waitForTimeout(ms);
	const to = await shownTime(page);
	return { rate: (to - from) / ((Date.now() - t0) / 1000), from, to };
}

/** True when two captures of the stage, `ms` apart, are pixel-identical (a frozen picture). */
export async function pictureFrozen(page: Page, ms: number): Promise<boolean> {
	const host = page.locator('.video-host').first();
	const a = await host.screenshot({ animations: 'disabled' });
	await page.waitForTimeout(ms);
	const b = await host.screenshot({ animations: 'disabled' });
	return a.equals(b);
}

export async function audioLevels(page: Page) {
	return page.evaluate(() =>
		(window as unknown as { __benchAudio: () => { left: number; right: number; sampleRate: number; state: string }[] }).__benchAudio()
	);
}

/** The loudest reading of each channel over `ms` (the beep lasts 150 ms of every second). */
export async function peakAudio(page: Page, ms: number) {
	let left = 0;
	let right = 0;
	let sampleRate = 0;
	const end = Date.now() + ms;
	while (Date.now() < end) {
		for (const t of await audioLevels(page)) {
			left = Math.max(left, t.left);
			right = Math.max(right, t.right);
			sampleRate = t.sampleRate;
		}
		await page.waitForTimeout(50);
	}
	return { left, right, sampleRate };
}

export async function workerPosts(page: Page): Promise<Record<string, number>> {
	return page.evaluate(() => ({ ...(window as unknown as { __benchPosts: Record<string, number> }).__benchPosts }));
}

export async function sessionPositions(page: Page): Promise<MediaPositionState[]> {
	return page.evaluate(() => [...(window as unknown as { __benchPositions: MediaPositionState[] }).__benchPositions]);
}

/** Opens the playback details (through the shortcuts panel, where its toggle lives); they stay
 * shown once the panel closes. */
export async function showDetails(page: Page) {
	await stage(page).hover();
	await stage(page).press('?');
	await page.getByRole('button', { name: 'Show playback details' }).click();
	await page.getByRole('dialog', { name: 'Keyboard shortcuts' }).getByRole('button', { name: 'Close' }).click();
	await expect(page.getByLabel('Playback details')).toBeVisible();
}

/** The playback details, label to value (the page's facts, then the engine's). */
export async function readDetails(page: Page): Promise<Record<string, string>> {
	return page.getByLabel('Playback details').evaluate((el) => {
		const o: Record<string, string> = {};
		for (const dt of el.querySelectorAll('dt')) o[dt.textContent?.trim() ?? ''] = dt.nextElementSibling?.textContent?.trim() ?? '';
		return o;
	});
}
