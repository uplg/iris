// The end-to-end playback bench (`bun run e2e`): real browsers against a real backend fed by
// generated clips. Chrome is the installed Google Chrome (Playwright's Chromium has no H.264 /
// HEVC / AAC); Firefox and WebKit are Playwright's builds. No desktop device descriptors: their
// user agent says Windows, and the engine picks by user agent (Gecko-on-macOS CRA splice, the
// Windows Chromium cue guard), so each browser keeps its own.
import { defineConfig, devices } from '@playwright/test';
import { BENCH_PORT } from './harness/catalog.ts';

export default defineConfig({
	testDir: './specs',
	outputDir: './.results',
	globalSetup: './harness/global-setup.ts',
	// one backend, one librqbit, real decoders: specs run one at a time
	workers: 1,
	fullyParallel: false,
	timeout: 180_000,
	expect: { timeout: 20_000 },
	retries: 0,
	reporter: [['list'], ['html', { outputFolder: './.report', open: 'never' }]],
	use: {
		baseURL: `http://127.0.0.1:${BENCH_PORT}`,
		trace: 'retain-on-failure',
		video: 'off',
		viewport: { width: 1280, height: 800 }
	},
	// each test names its browsers with a tag (`@chrome`, `@firefox`, `@webkit`, `@phone`)
	projects: [
		{
			name: 'chrome',
			grep: /@chrome\b/,
			use: {
				browserName: 'chromium',
				channel: 'chrome',
				viewport: { width: 1280, height: 800 }
			}
		},
		{
			name: 'firefox',
			grep: /@firefox\b/,
			use: {
				browserName: 'firefox',
				viewport: { width: 1280, height: 800 }
			}
		},
		{ name: 'webkit', grep: /@webkit\b/, use: { browserName: 'webkit', viewport: { width: 1280, height: 800 } } },
		// Zen is driven by puppeteer over WebDriver BiDi inside its specs (lib/zen.ts): the
		// project only selects them, its own Playwright browser is never launched
		{ name: 'zen', grep: /@zen\b/ },
		{
			name: 'phone',
			grep: /@phone\b/,
			use: {
				...devices['Pixel 7'],
				channel: 'chrome'
			}
		}
	]
});
