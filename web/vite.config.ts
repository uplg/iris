import path from "node:path";
import { readFileSync } from "node:fs";
import { execSync } from "node:child_process";
import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Bake the package.json `version` field into the bundle so the runtime
// `X-Iris-Client: web/X.Y.Z` header reflects exactly what shipped. Read
// at config-load time so the value freezes per build.
const pkg = JSON.parse(readFileSync(path.resolve(__dirname, "package.json"), "utf-8"));

// A build identity that changes on EVERY deploy — not just version bumps,
// since deploys often ship the same `version`. Resolution order:
//   1. `IRIS_WEB_BUILD_ID` env (the Docker build passes the git sha here),
//   2. the local git short hash (dev checkouts),
//   3. a build timestamp (Docker excludes `.git`, so this is the fallback —
//      still unique per build, so every deploy is detected).
// Combined with the version for readability, e.g. `0.4.0+a1b2c3d`.
function resolveBuildId(version: string): string {
  const explicit = process.env.IRIS_WEB_BUILD_ID?.trim();
  if (explicit) return `${version}+${explicit}`;
  try {
    const sha = execSync("git rev-parse --short HEAD", {
      cwd: __dirname,
      stdio: ["ignore", "pipe", "ignore"],
    })
      .toString()
      .trim();
    if (sha) return `${version}+${sha}`;
  } catch {
    /* not a git checkout (e.g. the Docker build) — fall through to a timestamp */
  }
  return `${version}+${Date.now().toString(36)}`;
}
const buildId = resolveBuildId(pkg.version);

// Emit `dist/version.json` carrying the build id. It lives at the dist root
// (NOT under `/assets/`), so the backend serves it `no-cache, must-revalidate`
// — the frontend polls it to detect a deploy and offer a reload.
function emitVersionJson(): Plugin {
  return {
    name: "iris-emit-version-json",
    generateBundle() {
      this.emitFile({
        type: "asset",
        fileName: "version.json",
        source: `${JSON.stringify({ buildId, version: pkg.version })}\n`,
      });
    },
  };
}

export default defineConfig({
  plugins: [react(), tailwindcss(), emitVersionJson()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "src"),
    },
  },
  define: {
    __IRIS_WEB_VERSION__: JSON.stringify(pkg.version),
    __IRIS_BUILD_ID__: JSON.stringify(buildId),
  },
  server: {
    proxy: {
      "/api": "http://localhost:8080",
    },
  },
});
