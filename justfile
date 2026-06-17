# Iris — task runner (just). Run `just` to list recipes.
#
# `just deploy` rebuilds + restarts, stamping the web bundle with the current
# git commit so already-open browser tabs detect the redeploy and offer a
# reload (web/src/components/UpdateBanner.tsx). `.git` is excluded from the
# Docker build context, so Vite CANNOT read the sha inside the build — it must
# be injected from the host. That's the whole point of the stamping; plain
# `docker compose up -d --build` still works (falls back to a build timestamp).

# Stamp the web bundle with the short commit (empty outside a git checkout).
export IRIS_WEB_BUILD_ID := `git rev-parse --short HEAD 2>/dev/null || true`

# List recipes.
default:
    @just --list

# Rebuild + restart (cloudflared profile, build-id stamped); extra flags pass through.
deploy *ARGS:
    docker compose {{ ARGS }} --profile cloudflared up -d --build

# --- Backend (Rust workspace) -----------------------------------------------

# Run the dev server on :8080.
run:
    cargo run -p iris-api

# Type-check the whole workspace.
check:
    cargo check --workspace --all-targets

# Clippy on everything (zero-warning bar).
clippy:
    cargo clippy --workspace --all-targets --no-deps

# Run all workspace tests.
test:
    cargo test --workspace

# --- Web (bun) --------------------------------------------------------------

# Vite dev server (proxies /api -> http://localhost:8080).
web-dev:
    cd web && bun run dev

# Production web build (tsc -b && vite build).
web-build:
    cd web && bun run build

# oxlint the web app.
web-lint:
    cd web && bun run lint

# Regenerate the OpenAPI spec (Rust handlers) + the web TS types.
gen:
    cd web && bun run gen-api

# Format Rust (cargo fmt) + web (oxfmt).
fmt:
    cargo fmt --all
    cd web && bun run format

# --- Android TV -------------------------------------------------------------

# Build the sideloadable release APK (R8); runs :app:openApiGenerate first.
apk:
    cd android-tv && ./gradlew :app:assembleRelease

# Build the debug APK.
apk-debug:
    cd android-tv && ./gradlew :app:assembleDebug

# Rebuild the native Media3 decoder AARs (after a media3 bump — long compile).
tv-aars:
    cd android-tv && rm -rf .ffmpeg-ext-build/media && ./scripts/build-ffmpeg-ext.sh && ./scripts/build-av1-ext.sh

# --- Gate -------------------------------------------------------------------

# Full local gate: clippy + tests + web lint/build + release APK.
verify: clippy test web-lint web-build apk
