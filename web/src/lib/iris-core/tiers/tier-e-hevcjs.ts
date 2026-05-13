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

async function ensureIntercept(): Promise<void> {
  if (!intercept) {
    // Lazy-load the lib so only Tier E sessions pay the ~70 KB cost.
    const mod = await import("@hevcjs/core");
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
