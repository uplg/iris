// Keeps a bench up between runs while writing specs: `bun e2e/harness/serve.ts`, then
// `IRIS_E2E_REUSE=1 bun run e2e …` in another shell. Ctrl-C tears it down.
import globalSetup from './global-setup.ts';

const teardown = await globalSetup();
console.log('[e2e] bench up; Ctrl-C to stop');
process.on('SIGINT', () => {
	void teardown().then(() => process.exit(0));
});
