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

# Rebuild + restart (cloudflared profile, build-id stamped); extra flags pass
# through. The token guard lives HERE (not as `:?` in docker-compose.yml)
# because Compose interpolates profile-disabled services too — a hard `:?`
# would break `just dev` on machines without the tunnel token.
deploy *ARGS:
    @[ -n "${CLOUDFLARE_TUNNEL_TOKEN:-}" ] || grep -q '^CLOUDFLARE_TUNNEL_TOKEN=..*' .env 2>/dev/null || { echo "deploy needs CLOUDFLARE_TUNNEL_TOKEN in .env (the cloudflared profile dials the tunnel with it)"; exit 1; }
    docker compose {{ ARGS }} --profile cloudflared up -d --build

# Local docker deploy (no tunnel/proxy profile), build-id stamped.
# `just dev` serves on :8080; `just dev 18080` picks another host port
# (container keeps 8080 internally — only the mapping changes).
dev port="8080":
    IRIS_BIND_PORT={{ port }} docker compose up -d --build

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

# The update channel is https://synthe.se/app-release.apk (Caddy on yuki
# serving /var/www/iris; https://uplg.xyz/app-release.* 301s there for the
# clients still pointing at the old host). The sidecar is derived from
# `versionName` in android-tv/app/build.gradle.kts so it cannot drift from
# the APK. Both files land under a temporary name and are renamed in place,
# APK first, so the sidecar never announces a version whose APK is not
# fully there yet.

# Publish the release APK from `just apk` + its version sidecar to yuki (ssh alias).
apk-push host="yuki" dir="/var/www/iris":
    #!/usr/bin/env bash
    set -euo pipefail
    apk=android-tv/app/build/outputs/apk/release/app-release.apk
    [ -f "$apk" ] || { echo "no release APK at $apk - run \`just apk\` first"; exit 1; }
    version=$(sed -nE 's/^[[:space:]]*versionName = "([0-9]+\.[0-9]+\.[0-9]+)".*/\1/p' android-tv/app/build.gradle.kts)
    [ -n "$version" ] || { echo "could not read versionName from android-tv/app/build.gradle.kts"; exit 1; }
    echo "pushing $apk ($(du -h "$apk" | cut -f1)) as version $version to {{ host }}:{{ dir }}"
    scp -q "$apk" "{{ host }}:{{ dir }}/app-release.apk.tmp"
    printf '%s\n' "$version" | ssh "{{ host }}" "cat > '{{ dir }}/app-release.version.tmp' \
      && chmod 644 '{{ dir }}/app-release.apk.tmp' '{{ dir }}/app-release.version.tmp' \
      && mv '{{ dir }}/app-release.apk.tmp' '{{ dir }}/app-release.apk' \
      && mv '{{ dir }}/app-release.version.tmp' '{{ dir }}/app-release.version'"
    echo "published: $(curl -fsS https://synthe.se/app-release.version) at https://synthe.se/app-release.apk"

# Rebuild the native Media3 decoder AARs (after a media3 bump — long compile).
tv-aars:
    cd android-tv && rm -rf .ffmpeg-ext-build/media && ./scripts/build-ffmpeg-ext.sh && ./scripts/build-av1-ext.sh

# --- Gate -------------------------------------------------------------------

# Full local gate: clippy + tests + web lint/build + release APK.
verify: clippy test web-lint web-build apk
