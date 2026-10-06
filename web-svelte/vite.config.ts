import { readFileSync } from 'node:fs';
import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { playwright } from '@vitest/browser-playwright';
import { defineConfig } from 'vitest/config';

// The version the `X-Iris-Client: web/<version>` header carries (@iris/api).
const pkg = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8')) as { version: string };

export default defineConfig({
	plugins: [
		sveltekit({
			compilerOptions: {
				// runes everywhere except in libraries
				runes: ({ filename }) => (filename.split(/[/\\]/).includes('node_modules') ? undefined : true)
			},
			// a pure SPA: the Rust backend serves build/ and falls back to index.html
			adapter: adapter({ fallback: 'index.html' }),
			// a deploy is noticed within a minute, taken at the next harmless moment
			// (+layout.svelte: a navigation, or the app seen again)
			version: { pollInterval: 60_000 },
			serviceWorker: { register: false }
		})
	],
	define: {
		__IRIS_WEB_VERSION__: JSON.stringify(pkg.version)
	},
	// pre-bundled up front: a test that imports it first otherwise races Vite's on-the-fly
	// optimisation and fails once on a cold cache
	optimizeDeps: { include: ['axe-core'] },
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
