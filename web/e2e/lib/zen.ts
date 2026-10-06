// Zen (the Firefox-based browser the household uses) driven over WebDriver BiDi: Playwright's
// `firefox` needs its own Juggler-patched build, so a stock Gecko is driven by puppeteer. The
// specs keep the bench's shape (`zen` fixture: a signed-in page, its console, the same probes).
import { existsSync, writeFileSync } from 'node:fs';
import { launch, type Browser, type Page } from 'puppeteer-core';
import { test as base, expect } from '@playwright/test';
import { CATALOG, type BenchState, type ClipKey } from '../harness/catalog.ts';
import { readState, type Console } from './bench.ts';

export { expect };

export const ZEN_PATH = process.env.IRIS_E2E_ZEN ?? '/Applications/Zen.app/Contents/MacOS/zen';
const ZEN_PREFS: Record<string, unknown> = {
	// the bench clicks Play itself; a muted-autoplay policy would still block the element's
	// own play() after a demotion
	'media.autoplay.default': 0,
	'media.autoplay.blocking_policy': 0
};

export interface Zen {
	page: Page;
	browser: Browser;
	logs: Console;
	state: BenchState;
	version: string;
}

export const test = base.extend<{ zen: Zen }>({
	// oxlint-disable-next-line no-empty-pattern -- Playwright reads the fixture's dependencies from this destructuring
	zen: async ({}, use, info) => {
		test.skip(!existsSync(ZEN_PATH), `no Zen at ${ZEN_PATH} (IRIS_E2E_ZEN)`);
		const state = readState();
		const browser = await launch({
			browser: 'firefox',
			executablePath: ZEN_PATH,
			headless: process.env.IRIS_E2E_HEADFUL !== '1',
			defaultViewport: { width: 1280, height: 800 },
			extraPrefsFirefox: ZEN_PREFS
		});
		const page = await browser.newPage();
		const lines: string[] = [];
		page.on('console', (m) => lines.push(`[${m.type()}] ${m.text()}`));
		page.on('pageerror', (e) => lines.push(`[pageerror] ${e instanceof Error ? e.message : String(e)}`));
		await page.goto(state.baseUrl);
		const status = await page.evaluate(
			async (email, password) =>
				(
					await fetch('/api/auth/login', {
						method: 'POST',
						headers: { 'Content-Type': 'application/json' },
						body: JSON.stringify({ email, password })
					})
				).status,
			state.email,
			state.password
		);
		expect(status, 'login').toBe(200);
		const version = await browser.version();
		try {
			await use({
				page,
				browser,
				logs: { lines, has: (re) => lines.some((l) => re.test(l)), matching: (re) => lines.filter((l) => re.test(l)) },
				state,
				version
			});
		} finally {
			writeFileSync(info.outputPath('console.txt'), lines.join('\n'));
			await info.attach('console', { path: info.outputPath('console.txt'), contentType: 'text/plain' });
			await browser.close();
		}
	}
});

async function call(page: Page, method: string, path: string, body?: unknown): Promise<number> {
	return page.evaluate(
		async (m, p, b) =>
			(await fetch(p, b === null ? { method: m } : { method: m, headers: { 'Content-Type': 'application/json' }, body: b })).status,
		method,
		path,
		body === undefined ? null : JSON.stringify(body)
	);
}

/** `openWatch` of the bench, on Zen. */
export async function zenWatch(zen: Zen, clip: ClipKey, opts: { tier?: string; resume?: number } = {}) {
	const { page, state } = zen;
	const path = `/api/torrents/${state.infohash[clip]}/files/0/progress`;
	if (opts.resume) {
		expect(
			await call(page, 'PUT', path, { position_seconds: opts.resume, duration_seconds: CATALOG[clip].duration, seek: true })
		).toBeLessThan(300);
	} else {
		await call(page, 'DELETE', path);
	}
	await page.goto(`${state.baseUrl}/watch/${state.infohash[clip]}/0${opts.tier ? `?tier=${opts.tier}` : ''}`);
	await page.waitForSelector('::-p-aria([name="Play, Space"][role="button"]), ::-p-aria([name="Pause, Space"][role="button"])', {
		timeout: 60_000
	});
}

/** Starts playback through the chrome's button, when it says Play. */
export async function zenPlay(zen: Zen) {
	const { page } = zen;
	await page.hover('.stage');
	const play = await page.$('::-p-aria([name="Play, Space"][role="button"])');
	if (play) await play.click();
}

export async function zenVideo(zen: Zen) {
	return zen.page.evaluate(() => {
		const v = document.querySelector<HTMLVideoElement>('.video-host video');
		if (!v) return null;
		const ranges: [number, number][] = [];
		for (let i = 0; i < v.buffered.length; i++) ranges.push([v.buffered.start(i), v.buffered.end(i)]);
		return {
			currentTime: v.currentTime,
			paused: v.paused,
			error: v.error ? `${v.error.code} ${v.error.message}` : null,
			buffered: ranges,
			mozHasAudio: (v as HTMLVideoElement & { mozHasAudio?: boolean }).mozHasAudio ?? null
		};
	});
}

/** Polls `probe` until it holds, like `expect.poll`. */
export async function zenUntil(what: string, timeoutMs: number, probe: () => Promise<boolean>) {
	const end = Date.now() + timeoutMs;
	while (Date.now() < end) {
		if (await probe()) return;
		await new Promise((r) => setTimeout(r, 250));
	}
	throw new Error(`timed out (${timeoutMs} ms) waiting for ${what}`);
}

export async function zenKey(zen: Zen, key: 'ArrowLeft' | 'ArrowRight') {
	await zen.page.focus('.stage');
	await zen.page.keyboard.press(key);
}
