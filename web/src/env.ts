// Values the shell needs before any script runs (app.html's %sveltekit.env.*%), also importable
// from `$app/env/public`: each is said here once.
import { defineEnvVars } from '@sveltejs/kit/env';
import { STORAGE } from './lib/storage.ts';

export const variables = defineEnvVars({
	/** The chosen theme, read before first paint and by `ui`. */
	PUBLIC_THEME_KEY: { public: true, static: true, schema: () => STORAGE.theme },
	/** The accounts this browser already offered a passkey to (asked once per device). */
	PUBLIC_PASSKEY_OFFERED_KEY: { public: true, static: true, schema: () => STORAGE.passkeyOffered }
});
