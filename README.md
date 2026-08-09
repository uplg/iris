# Iris

Self-hosted "micro-Netflix": aggregate searches across torrent trackers, stream the result inside a React video player, seed the rest, and reclaim disk space when it runs low.

## Stack

- **Backend**: Rust 2024 workspace, Axum, SQLite (sqlx), librqbit, ffmpeg pipeline.
- **Frontend**: React 19 + Vite + Tailwind 4 + shadcn/ui, served by bun.
- **Android TV**: Compose-for-TV + Media3/ExoPlayer (`android-tv/`), full feature
  parity with the web app, DTOs generated from the OpenAPI spec, self-hosted APK
  updates from the in-app Settings screen.
- **Auth**: invitation-only, JWT in HTTP-only cookies, argon2id passwords.

## Local dev

- Via host :

```sh
# 1. Backend
cp config/config.toml.example config/config.toml
# edit auth.jwt_secret + auth.bootstrap_admin
cargo run -p iris-api

# 2. Frontend (separate terminal)
cd web
bun install
bun run dev
# Vite dev server proxies /api -> http://localhost:8080
```

- Via docker :

```sh
docker compose up -d --build
```

Visit http://localhost:5173 (or :8080 if using docker), log in with the bootstrap admin, generate an
invitation in /admin, and use it from /register.

## Production (Docker)

```sh
cp .env.example .env
# fill everything
just deploy
# or if you don't have just (with cloudflare setup)
docker compose --profile cloudflared up --build -d
```

See [deployment.md](./docs/DEPLOYMENT.md) for server setup.

## Android TV app

### Install on a TV / box

- Install Downloader by AFTVNews on Google play store.
- Type the code : **8737385** or use this url : https://uplg.xyz/app-release.apk
- Install (allow unknown sources if needed)

(New updates can be done after using Iris settings screen > Update app.)

**Note**: The app now works on Android (mobile), but is a bit more experimental.

### Develop / build

Open `android-tv/` in Android Studio (Hedgehog+). AGP 9 enables Kotlin
automatically no extra plugin setup.

```sh
just apk
# also runs :app:openApiGenerate — Kotlin DTOs are generated from
# web/openapi.json, never hand-written
# or if you don't have just
cd android-tv
./gradlew :app:assembleRelease
```

Two gotchas worth knowing before touching it:

- The ffmpeg/AV1 decoder AARs in `app/libs/` are hand-built native
  extensions version-coupled to the exact `media3` release — after a
  media3 bump, rebuild them with `scripts/build-ffmpeg-ext.sh` and
  `scripts/build-av1-ext.sh`.
- A release upload is TWO files: `app-release.apk` **and** the
  `app-release.version` sidecar (plain semver, one line) that powers the
  "Update available" card in Settings.
