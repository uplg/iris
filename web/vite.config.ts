import path from "node:path";
import { readFileSync } from "node:fs";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Bake the package.json `version` field into the bundle so the runtime
// `X-Iris-Client: web/X.Y.Z` header reflects exactly what shipped. Read
// at config-load time so the value freezes per build.
const pkg = JSON.parse(readFileSync(path.resolve(__dirname, "package.json"), "utf-8"));

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "src"),
    },
  },
  define: {
    __IRIS_WEB_VERSION__: JSON.stringify(pkg.version),
  },
  server: {
    proxy: {
      "/api": "http://localhost:8080",
    },
  },
});
