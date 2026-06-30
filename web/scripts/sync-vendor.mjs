// Vendored static assets — copies the WASM workers / fonts that ship inside
// node_modules into `public/` so Vite serves them at a stable URL (the
// libass/libpgs/hevc.js workers fetch their sibling `.wasm` by relative path,
// so they can't go through Vite's hashed-asset pipeline).
//
// Runs on `predev` / `prebuild`, so the copies are ALWAYS regenerated from the
// installed package version — the `public/{libass,hevcjs,libpgs}/` dirs are
// gitignored on purpose, never committed, so they can't drift from the lockfile
// (a stale committed `transcode-worker.js` once shipped against a newer decoder
// — this script exists to make that impossible).
//
// `--check` mode (CI guard): verify the copies match node_modules without
// writing, exit non-zero on any drift.
//
// Each entry pins the package's expected MAJOR version as a tripwire: a major
// bump fails the build until someone reviews the loader (the worker↔client
// protocol can change across majors) and bumps `expectMajor` deliberately.

import { copyFileSync, existsSync, mkdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const WEB_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const NODE_MODULES = join(WEB_ROOT, "node_modules");
const PUBLIC = join(WEB_ROOT, "public");

/** @type {{pkg: string, expectMajor: number, files: [string, string][]}[]} */
const VENDOR = [
  {
    // libass-wasm — ASS/SSA subtitle overlay (subtitles-octopus worker).
    // Loader: src/lib/iris-core/subs/* via /libass/*.
    pkg: "@jellyfin/libass-wasm",
    expectMajor: 4,
    files: [
      ["dist/js/subtitles-octopus.js", "libass/subtitles-octopus.js"],
      ["dist/js/subtitles-octopus-worker.js", "libass/subtitles-octopus-worker.js"],
      ["dist/js/subtitles-octopus-worker.wasm", "libass/subtitles-octopus-worker.wasm"],
      ["dist/js/subtitles-octopus-worker-legacy.js", "libass/subtitles-octopus-worker-legacy.js"],
      ["dist/js/default.woff2", "libass/default.woff2"],
    ],
  },
  {
    // hevc.js — Tier E WASM HEVC transcode. The worker and the main-thread
    // client share a versioned protocol, so the worker MUST match the
    // installed decoder (this is the file that previously drifted).
    // Loader: src/lib/iris-core/tiers/tier-e-hevcjs.ts via /hevcjs/*.
    pkg: "@hevcjs/core",
    expectMajor: 1,
    files: [
      ["dist/wasm/hevc-decode.js", "hevcjs/hevc-decode.js"],
      ["dist/wasm/hevc-decode.wasm", "hevcjs/hevc-decode.wasm"],
      ["dist/transcode-worker.js", "hevcjs/transcode-worker.js"],
    ],
  },
  {
    // libpgs — PGS (BluRay) subtitle overlay worker.
    // Loader: src/lib/iris-core/subs/pgs-overlay.ts via /libpgs/*.
    pkg: "libpgs",
    expectMajor: 0,
    files: [["dist/libpgs.worker.js", "libpgs/libpgs.worker.js"]],
  },
  {
    // libav.js — the `-default` WASM build: the fallback audio decoder for
    // Tier B when the custom AC-3/E-AC-3/DTS `-iris` variant (built in the
    // Dockerfile via emscripten, NOT shipped on npm) isn't present. We only
    // sync `-default`; the Docker-built `-iris` files sit alongside it and are
    // left untouched (copyFileSync never wipes the dir).
    // Loader: src/lib/iris-core/decode/libav-audio-decoder.ts via /libavjs/*.
    //
    // The version is embedded in the filename, so a libav.js bump makes these
    // source paths vanish — the existence assertion then fails the build until
    // both this manifest AND the loader's hardcoded filename are updated to
    // the new version together (intentional: they must move in lockstep).
    pkg: "libav.js",
    expectMajor: 6,
    files: [
      ["dist/libav-6.9.8.1-default.wasm.js", "libavjs/libav-6.9.8.1-default.wasm.js"],
      ["dist/libav-6.9.8.1-default.wasm.mjs", "libavjs/libav-6.9.8.1-default.wasm.mjs"],
      ["dist/libav-6.9.8.1-default.wasm.wasm", "libavjs/libav-6.9.8.1-default.wasm.wasm"],
    ],
  },
];

const checkOnly = process.argv.includes("--check");
const problems = [];

for (const { pkg, expectMajor, files } of VENDOR) {
  const pkgJsonPath = join(NODE_MODULES, pkg, "package.json");
  if (!existsSync(pkgJsonPath)) {
    problems.push(`${pkg}: not installed (run \`bun install\`)`);
    continue;
  }
  const version = JSON.parse(readFileSync(pkgJsonPath, "utf8")).version;
  const major = Number(version.split(".")[0]);
  if (major !== expectMajor) {
    problems.push(
      `${pkg}@${version}: major ${major} ≠ expected ${expectMajor} — review the ` +
        `loader (the worker↔client protocol may have changed) then bump ` +
        `\`expectMajor\` in web/scripts/sync-vendor.mjs.`,
    );
    continue;
  }

  for (const [srcRel, destRel] of files) {
    const src = join(NODE_MODULES, pkg, srcRel);
    const dest = join(PUBLIC, destRel);
    if (!existsSync(src)) {
      problems.push(
        `${pkg}: source \`${srcRel}\` missing — its dist layout changed in ${version}.`,
      );
      continue;
    }
    if (checkOnly) {
      if (!existsSync(dest) || !readFileSync(src).equals(readFileSync(dest))) {
        problems.push(`drift: public/${destRel} ≠ ${pkg}@${version} (run \`bun run sync-vendor\`)`);
      }
    } else {
      mkdirSync(dirname(dest), { recursive: true });
      copyFileSync(src, dest);
    }
  }

  if (!problems.length) {
    console.log(`vendor: ${pkg}@${version} (${files.length} files)`);
  }
}

if (problems.length) {
  console.error(`\nsync-vendor ${checkOnly ? "check" : "sync"} failed:`);
  for (const p of problems) console.error(`  • ${p}`);
  process.exit(1);
}
console.log(`vendor: ${checkOnly ? "all copies match node_modules" : "synced into public/"}`);
