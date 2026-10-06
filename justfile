# Iris — task runner (just). Run `just` to list recipes.
#
# `just deploy` rebuilds + restarts, stamping the web bundle with the current
# git commit so already-open browser tabs detect the redeploy and reload at
# the next navigation (SvelteKit's `version`, web/vite.config.ts). `.git` is excluded from the
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
# `just dev` serves on 127.0.0.1:8080; `just dev 18080` picks another host
# port (container keeps 8080 internally — only the mapping changes). The
# port mapping lives in docker-compose.dev.yml only: production publishes none.
dev port="8080":
    IRIS_BIND_PORT={{ port }} docker compose -f docker-compose.yml -f docker-compose.dev.yml up -d --build

# Pull a consistent copy of the prod DB into ~/iris-prod-rehearsal/incoming (VACUUM INTO, prod keeps running).
rehearsal-fetch host="yuki":
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p ~/iris-prod-rehearsal/incoming
    ssh {{ host }} 'p=$(docker volume inspect iris_iris-data -f "{{{{.Mountpoint}}"); rm -f /tmp/iris-prod.db; sqlite3 "file:$p/iris.db?mode=ro" "VACUUM INTO '"'"'/tmp/iris-prod.db'"'"'"'
    ssh {{ host }} 'zstd -q -3 -T0 -c /tmp/iris-prod.db && rm -f /tmp/iris-prod.db' | zstd -q -d > ~/iris-prod-rehearsal/incoming/iris-prod.db
    sqlite3 ~/iris-prod-rehearsal/incoming/iris-prod.db "PRAGMA quick_check;"

# The image is `iris:rehearsal` (`docker build -t iris:rehearsal .`). No
# librqbit/ state is copied, so the engine holds no torrent and nothing
# announces. The data lives in a named volume like prod: SQLite's WAL -shm
# mmap over a Docker Desktop bind mount SIGBUSes as soon as the host touches
# the file, so query it from a container:
# `docker run --rm -v iris-rehearsal-data:/data iris-sqlite /data/iris.db "<sql>"`.
# `docker volume rm iris-rehearsal-data` starts over from the copy.
# Local iris-prod on a prod DB copy, http://localhost:18080 (`mode=lan`: a TV box can pair).
rehearsal db mode="local":
    #!/usr/bin/env bash
    set -euo pipefail
    src="{{ db }}"
    mode="{{ mode }}"
    repo=$PWD
    vol=iris-rehearsal-data
    port=18080
    if [ "$mode" = lan ]; then
      ip=$(ipconfig getifaddr en0 || ipconfig getifaddr en1)
      bind=0.0.0.0
      public="http://$ip:$port"
    else
      bind=127.0.0.1
      public="http://localhost:$port"
    fi
    docker image inspect iris-sqlite >/dev/null 2>&1 ||
      printf 'FROM alpine:3\nRUN apk add --no-cache sqlite\nENTRYPOINT ["sqlite3"]\n' | docker build -q -t iris-sqlite - >/dev/null
    # a clean stop: librqbit and SQLite save their state (rm -f alone is a SIGKILL)
    docker stop -t 30 iris-rehearsal >/dev/null 2>&1 || true
    docker rm iris-rehearsal >/dev/null 2>&1 || true
    # one secret for the rehearsal's life, so a restart keeps everyone signed in
    secret_file=~/iris-prod-rehearsal/jwt-secret
    [ -s "$secret_file" ] || { mkdir -p ~/iris-prod-rehearsal; openssl rand -base64 48 > "$secret_file"; }
    if ! docker volume inspect $vol >/dev/null 2>&1; then
        sqlite3 "$src" "PRAGMA quick_check;" | grep -qx ok
        docker volume create $vol >/dev/null
        docker run --rm -v "$(dirname "$src"):/in:ro" -v $vol:/data alpine:3 \
          sh -c "cp /in/$(basename "$src") /data/iris.db && chown -R 1001:1001 /data 2>/dev/null; true"
        # No librqbit/ persistence is ever copied: the engine boots with zero torrents, so
        # nothing announces to a tracker. A torrent grabbed later from the UI is the tester's.
        if docker run --rm -v $vol:/data alpine:3 sh -c '[ -n "$(ls -A /data/librqbit 2>/dev/null)" ]'; then
          echo "refusing: /data/librqbit is not empty on a fresh copy (a torrent would announce)" >&2
          exit 1
        fi
    fi
    docker run -d --name iris-rehearsal \
      --env-file "$repo/.env" \
      -e IRIS_CONFIG=/srv/iris/config/config.toml \
      -e IRIS_SERVER__BIND=0.0.0.0:8080 \
      -e IRIS_SERVER__WEB_DIST=/srv/iris/web \
      -e IRIS_SERVER__PUBLIC_URL="$public" \
      -e IRIS_STORAGE__DATA_DIR=/data \
      -e IRIS_STORAGE__DOWNLOAD_DIR=/data/downloads \
      -e IRIS_AUTH__JWT_SECRET="$(cat "$secret_file")" \
      -e RUST_LOG=info,iris_api=debug,tower_http=info,html5ever=error \
      -v "$repo/config:/srv/iris/config:ro" \
      -v $vol:/data \
      -p "$bind:$port:8080" \
      iris:rehearsal >/dev/null
    echo "iris-rehearsal → $public   (logs: docker logs -f iris-rehearsal)"

# Backend (Rust workspace)

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

# Web (bun)

# Vite dev server (proxies /api -> http://localhost:8080).
web-dev:
    cd web && bun run dev

# Production web build (SvelteKit, adapter-static → web/build).
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

# Android TV

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

# Gate

# Full local gate: clippy + tests + web lint/build + release APK.
verify: clippy test web-lint web-build apk
