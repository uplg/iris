/**
 * Tier E — hevc.js MSE intercept. HEVC bitstream is transcoded to
 * H.264 in a worker; the browser's MSE only ever sees H.264.
 *
 * The flow re-uses Tier B's Mediabunny → fMP4 → MSE plumbing
 * verbatim — what changes is that `installMSEIntercept()` is called
 * before MediaSource is constructed, which transparently routes any
 * `hev1.*` / `hvc1.*` SourceBuffer through the WASM HEVC decoder +
 * H.264 encoder.
 *
 * Gated by `pickTier` to:
 *   - codec = HEVC (otherwise Tier B already works)
 *   - height ≤ 1080 (hevc.js does ~21 fps on 4K, not real-time)
 *   - browser ∈ Chromium-family (Firefox lacks WebCodecs H.264 encode
 *     in some versions; conservative)
 */

import type { EngineMount } from "../engine";

const WASM_URL = "/hevcjs/hevc-decode.js";
const WASM_BINARY_URL = "/hevcjs/hevc-decode.wasm";
const WORKER_URL = "/hevcjs/transcode-worker.js";

let intercept: { install: () => void; uninstall: () => void } | null = null;
let installed = false;
let installCount = 0;

/** Publish the WASM decoder factory as `globalThis.HEVCDecoderModule`.
 *
 *  hevc.js resolves its decoder like this:
 *
 *      if (typeof globalThis.HEVCDecoderModule === "function") { … }
 *      const mod = await import(wasmUrl);
 *      const fn = mod.default ?? mod;
 *
 *  The second path needs `hevc-decode.js` to be an ES module. The file the
 *  package ships — and that `sync-vendor` copies into `public/hevcjs/` — is the
 *  UMD build: it ends in `module.exports = HEVCDecoderModule` / `define([...])`
 *  and exports nothing to ESM. A browser `import()` of it therefore yields a
 *  namespace with no `default`, and hevc.js dies on
 *  `(mod.default ?? mod) is not a function` — which is why Tier E never
 *  started, in Firefox and in Chromium alike.
 *
 *  A classic `<script>` is what a UMD bundle is built for: it assigns the
 *  global, and hevc.js then takes its first branch and never reaches the
 *  import. Loading it here rather than vendoring an ESM variant keeps
 *  `sync-vendor` copying exactly what the package publishes. */
function ensureDecoderGlobal(): Promise<void> {
  const g = globalThis as { HEVCDecoderModule?: unknown };
  if (typeof g.HEVCDecoderModule === "function") return Promise.resolve();
  const existing = document.querySelector<HTMLScriptElement>(`script[src="${WASM_URL}"]`);
  const el = existing ?? document.createElement("script");
  const done = new Promise<void>((resolve, reject) => {
    el.addEventListener("load", () => resolve(), { once: true });
    el.addEventListener("error", () => reject(new Error(`Tier E: failed to load ${WASM_URL}`)), {
      once: true,
    });
  });
  if (!existing) {
    el.src = WASM_URL;
    el.async = true;
    document.head.appendChild(el);
  }
  return done;
}

async function ensureIntercept(): Promise<void> {
  if (!intercept) {
    // Lazy-load the lib so only Tier E sessions pay the ~70 KB cost.
    const mod = await import("@hevcjs/core");
    await ensureDecoderGlobal();
    intercept = {
      install: () =>
        mod.installMSEIntercept({
          wasmUrl: WASM_URL,
          wasmBinaryUrl: WASM_BINARY_URL,
          workerUrl: WORKER_URL,
          logLevel: "warn",
        }),
      uninstall: () => mod.uninstallMSEIntercept(),
    };
  }
  if (!installed) {
    intercept.install();
    installed = true;
  }
  installCount += 1;
}

function releaseIntercept(): void {
  if (!intercept || !installed) return;
  installCount = Math.max(0, installCount - 1);
  // Don't uninstall while the SourceBuffer still references the
  // intercept's proxy machinery — uninstall on the last release.
  if (installCount === 0) {
    intercept.uninstall();
    installed = false;
  }
}

export const mountTierE: EngineMount = async (opts) => {
  await ensureIntercept();
  // Delegate the rest to Tier B — the intercept transparently catches
  // hev1/hvc1 SourceBuffer creations and routes through HEVC→H.264.
  const { mountTierB } = await import("./tier-b-mse");
  let handle;
  try {
    handle = await mountTierB(opts);
  } catch (e) {
    releaseIntercept();
    throw e;
  }
  const originalDispose = handle.dispose;
  handle.dispose = async () => {
    try {
      await originalDispose();
    } finally {
      releaseIntercept();
    }
  };
  return handle;
};
