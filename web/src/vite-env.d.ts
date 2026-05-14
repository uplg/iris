/// <reference types="vite/client" />

// Baked into the bundle by `vite.config.ts`'s `define` block — reflects
// the `web/package.json` version at build time. Used by the
// `X-Iris-Client` request header so the server can log + gate by
// version. Frozen per build; refreshing the page after a backend
// deploy is what pulls the new bundle (and thus the new version).
declare const __IRIS_WEB_VERSION__: string;
