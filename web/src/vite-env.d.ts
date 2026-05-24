/// <reference types="vite/client" />

// Baked into the bundle by `vite.config.ts`'s `define` block — reflects
// the `web/package.json` version at build time. Used by the
// `X-Iris-Client` request header so the server can log + gate by
// version. Frozen per build; refreshing the page after a backend
// deploy is what pulls the new bundle (and thus the new version).
declare const __IRIS_WEB_VERSION__: string;

// Per-build identity (`<version>+<git-sha|timestamp>`), baked by
// `vite.config.ts` and also emitted to `dist/version.json`. The frontend
// polls that file and, when it differs from this baked value, knows a deploy
// happened and offers a reload. Changes on EVERY build, unlike the version.
declare const __IRIS_BUILD_ID__: string;
