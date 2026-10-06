/// <reference types="vite/client" />

// `__IRIS_WEB_VERSION__` is declared by @iris/api (packages/iris-api/src/globals.d.ts).

// Per-build identity (`<version>+<git-sha|timestamp>`), baked by
// `vite.config.ts` and also emitted to `dist/version.json`. The frontend
// polls that file and, when it differs from this baked value, knows a deploy
// happened and offers a reload. Changes on EVERY build, unlike the version.
declare const __IRIS_BUILD_ID__: string;
