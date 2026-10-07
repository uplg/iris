import { execSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { playwright } from '@vitest/browser-playwright';
import { defineConfig } from 'vitest/config';
import type { Plugin } from 'vite';

// The version the `X-Iris-Client: web/<version>` header carries (@iris/api).
const pkg = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8')) as { version: string };

// The deploy's identity (`1.5.0+a1b2c3d`), so an open tab notices a redeploy of the same version:
// the Docker build passes the sha (its context has no .git), a checkout reads it, else the time.
function buildId(): string {
	const given = process.env.IRIS_WEB_BUILD_ID?.trim();
	if (given) return `${pkg.version}+${given}`;
	try {
		return `${pkg.version}+${execSync('git rev-parse --short HEAD', { stdio: ['ignore', 'pipe', 'ignore'] })
			.toString()
			.trim()}`;
	} catch {
		return `${pkg.version}+${Date.now().toString(36)}`;
	}
}

const BUILD_ID = buildId();

// `/version.json`, as the React app before 1.6.0 polls it (`{ buildId }`): a tab left open on
// that bundle reads a new id and offers its reload. Without the file the SPA fallback answered
// HTML, which the old tab took for "no update" and stayed on the old app.
function legacyVersionJson(): Plugin {
	return {
		name: 'iris-legacy-version-json',
		apply: 'build',
		applyToEnvironment: (env) => env.name === 'client',
		generateBundle() {
			this.emitFile({
				type: 'asset',
				fileName: 'version.json',
				source: `${JSON.stringify({ buildId: BUILD_ID, version: pkg.version })}\n`
			});
		}
	};
}

export default defineConfig({
	plugins: [
		legacyVersionJson(),
		sveltekit({
			compilerOptions: {
				// runes everywhere except in libraries
				runes: ({ filename }) => (filename.split(/[/\\]/).includes('node_modules') ? undefined : true)
			},
			// a pure SPA: the Rust backend serves build/ and falls back to index.html
			adapter: adapter({ fallback: 'index.html' }),
			// a deploy is noticed within a minute, taken at the next harmless moment
			// (+layout.svelte: a navigation, or the app seen again)
			version: { name: BUILD_ID, pollInterval: 60_000 },
			serviceWorker: { register: false }
		})
	],
	define: {
		__IRIS_WEB_VERSION__: JSON.stringify(pkg.version)
	},
	// pre-bundled up front: a test that imports it first otherwise races Vite's on-the-fly
	// optimisation and fails once on a cold cache
	optimizeDeps: { include: ['axe-core'] },
	// hls.js (~580 kB) is the largest engine chunk, loaded only when Tier F plays
	build: { chunkSizeWarningLimit: 600 },
	test: {
		expect: { requireAssertions: true },
		restoreMocks: true,
		unstubGlobals: true,
		projects: [
			{
				// components and rune modules: a real browser, as they run
				extends: true,
				test: {
					name: 'browser',
					include: ['src/**/*.svelte.test.ts'],
					setupFiles: ['src/test-setup.ts'],
					browser: { enabled: true, provider: playwright(), headless: true, instances: [{ browser: 'chromium' }] }
				}
			},
			{
				// plain modules: Node
				extends: true,
				test: {
					name: 'node',
					environment: 'node',
					// the shared packages' plain modules are tested here too
					include: ['src/**/*.test.ts', '../packages/*/src/**/*.test.ts'],
					exclude: ['src/**/*.svelte.test.ts']
				}
			}
		]
	},
	server: {
		proxy: {
			// IRIS_API_PROXY targets a backend on another port (docker owning 8080)
			'/api': process.env.IRIS_API_PROXY ?? 'http://localhost:8080'
		}
	}
});
