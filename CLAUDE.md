# Iris — agent reference

Self-hosted "micro-Netflix": aggregate searches across torrent trackers,
stream the result inside a React video player, seed the rest, reclaim disk
when it runs low. Invitation-only, single-tenant household.

## Absolute rules

Three project-wide rules — never relax them without explicit user approval.

1. **Clippy + tests, zero tolerance.** `cargo clippy --workspace --all-targets --no-deps`
   must report zero warnings and `cargo test` must be green **at every
   step**, not just at the end. If a crate you touch has pre-existing
   warnings, fix them in the same pass. Frontend equivalent: `oxlint`
   clean, `tsc -b` clean.
2. **Ask before adding any timer on the web target.** No `setTimeout`,
   `setInterval`, `queueMicrotask`, `requestAnimationFrame`-used-as-delay,
   or `Promise.resolve().then()` defer hacks in `web/` without explicit
   user approval. They cause subtle bugs (StrictMode double-fire, stale
   closures, Suspense reorderings). Prefer event-driven primitives
   (`addEventListener`, `IntersectionObserver`, React Query's
   `refetchInterval` / `staleTime`). Rust `tokio::time::*` and Android
   timers are not covered by this rule.
3. **Always pin the latest stable.** When adding any dep (Rust, npm,
   gradle catalog), check the current stable on the registry and pin
   that. If you notice drift between fresh and stale versions in the
   same change, propose an upgrade pass instead of mixing eras. No
   alphas / RCs unless the user opts in (see Cargo.toml notes for
   `argon2 0.6 is rc`, `sqlx 0.9 is alpha`).

## Stack snapshot

| Layer       | Stack                                                                              |
| ----------- | ---------------------------------------------------------------------------------- |
| Backend     | Rust 2024 workspace (edition 2024, rust-version 1.85), Axum 0.8, sqlx 0.9 / SQLite |
| Torrent     | librqbit 9.0.0.rc-0                                                                |
| Media       | ffmpeg-driven remuxer + HLS manifest builder + JS subtitle pipeline                |
| Auth        | JWT in HttpOnly cookies, argon2id, invitation-only registration                    |
| Frontend    | React 19, Vite 8, Tailwind 4, shadcn/ui (radix), TanStack Query, Tanstack Router   |
| Lint/format | oxlint + oxfmt (NOT eslint/prettier)                                               |
| Package mgr | **bun** — never npm/pnpm/yarn                                                      |
| Android TV  | Compose-for-TV, Media3 (ExoPlayer), AGP 9, Kotlin via AGP built-in                 |
| TMDB        | Movie/series metadata + SCENE-name resolution pipeline                             |

## Build & dev commands

```bash
# Backend (in repo root)
cargo run -p iris-api                     # dev server on :8080
cargo check --workspace --all-targets     # always before declaring done
cargo clippy --workspace --all-targets --no-deps  # zero-warning bar
cargo test -p <crate> --lib               # unit tests per crate

# Frontend (in web/)
bun install
bun run dev          # Vite proxies /api → http://localhost:8080
bun run build        # tsc -b && vite build
bun run lint         # oxlint (NOT eslint)
bun run format       # oxfmt

# Android TV (open android-tv/ in Android Studio Hedgehog+)
# AGP 9 enables Kotlin automatically — no kotlin-android plugin
# compileSdk/targetSdk 37 — targetSdk bumps need a real-device smoke test.
./gradlew :app:assembleRelease            # also runs :app:openApiGenerate (DTO codegen)

# Docker prod
docker compose up --build -d
```

## OpenAPI contract (BE ↔ web ↔ TV)

`web/openapi.json` (utoipa-derived, `crates/iris-api/src/openapi.rs`) is the
single source of truth for the request/response shapes. **Nothing hand-maintains
DTOs** — every client generates them:

- **Backend** emits the spec: `bun run gen-api` (or `cargo run -p iris-api
--bin gen-openapi -- --write`). The `committed_spec_is_current` test fails
  until you regenerate + commit `web/openapi.json`.
- **Web** generates TS types: `openapi-typescript` → `src/lib/api-types.ts`
  (auto via `predev`/`prebuild`). `api.ts` wraps them with the fetch client.
  The generator lives in its own mini-package (`web/tools/api-gen/`) pinned
  to TypeScript 6: it needs the TS compiler API (`ts.factory`), which the
  TS 7 native compiler used by the main build no longer ships. Don't
  "unify" the two TypeScript versions.
- **Android TV** generates `@Serializable` models: the `org.openapi.generator`
  Gradle plugin (`openApiGenerate` task, wired before compile via the AGP
  Variant API) → `app/build/generated/openapi/…/data/` in package
  `studio.kahn.iris.tv.data`. Models only — `IrisApi.kt` (the Retrofit
  interface) + the OkHttp wiring stay hand-written. Generator flags that
  matter: `serializationLibrary=kotlinx_serialization`,
  `enumUnknownDefaultCase=true` (unknown enum variant → fallback, never throws —
  the backward-compat rule), `generateOneOfAnyOfWrappers=true` (turns the
  tagged unions into working kotlinx sealed interfaces).

**Discriminated unions:** utoipa renders `#[serde(tag = "…")]` enums
(`LibraryResponse`, `PollResponse`) as a bare `oneOf` with NO discriminator,
which both generators mishandle. `promote_tagged_union` in `openapi.rs`
rewrites them (JSON-level, Rust types untouched → wire-identical) into named
`$ref` variants + an OpenAPI `discriminator`. **Keep that list in sync with the
tagged response enums.**

The generated Kotlin types are stricter than loose hand-written models
(`MediaKind`/`TmdbKind`/`EpisodeStatus`/`TorrentState` enums not `String`,
`UUID` not `String`, `Long` not `Int`, real nullability) — drift surfaces as
compile errors, which is the point. A field the TV used but the spec lacks
(e.g. `CatalogCard.new_count`) is genuine drift: add it to the Rust type, not a
hand-patched Kotlin model.

### Media3 decoder AARs are version-coupled

`app/libs/lib-decoder-{ffmpeg,av1}-<media3>-release.aar` are hand-built native
extensions (not on Maven) and **must match the `media3` version exactly** or
they crash at runtime. After bumping `media3` in `libs.versions.toml`:
`rm -rf android-tv/.ffmpeg-ext-build/media` (forces a re-clone at the new tag),
then run `scripts/build-ffmpeg-ext.sh` + `scripts/build-av1-ext.sh`. The av1
script re-applies `scripts/dav1d-10bit-surface.patch` — if it no longer applies
to the new Media3 source, regenerate it (else 10-bit AV1 breaks).

## Core architectural patterns

### Provider abstraction (`crates/iris-providers/src/lib.rs:22-47`)

Every tracker implements `SearchProvider`:

```rust
async fn search(&self, q: &SearchQuery) -> Result<ProviderPage>;
async fn resolve(&self, external_id: &str) -> Result<TorrentSource>;
async fn featured_movies(&self) -> Result<Vec<SearchResult>>;   // optional
async fn featured_series(&self) -> Result<Vec<SearchResult>>;   // optional
async fn details(&self, external_id: &str) -> Result<Option<TorrentDetails>>;  // optional
```

`ProviderRegistry::search_all` fans every query out in parallel via
`FuturesUnordered`; a failing provider becomes a metadata error entry,
not a 500.

**Adding a new tracker:**

1. New module under `crates/iris-providers/src/<name>.rs` implementing
   `SearchProvider`.
2. Register the `kind` in `registry.rs::build_provider`.
3. Add `pub mod <name>;` in `src/lib.rs`.
4. Add a `[[providers]]` entry to `config/providers.toml`.

If the tracker exposes a Torznab API, **compose `TorznabProvider`** from
the new module — don't reimplement the wire format. `c411.rs` is the
reference example (Torznab search/resolve + custom `/api/homepage`
featured carousel built from JSON).

### Torznab spec coverage

`torznab.rs` implements Torznab 1.3 search + resolve:

- `t=search` / `t=movie` / `t=tvsearch` with the right `cat` mapping
- RSS-with-`torznab:attr` envelope parsed via quick-xml event reader
- Per-process FIFO link cache (cap 4096) lets `resolve()` find the
  indexer-signed download URL captured during a previous `search()`
- Magnet bodies (returned by indexers when bandwidth credit is out)
  are surfaced as `TorrentSource::Magnet` instead of being treated as
  corrupted .torrent
- `cache_download_url()` is `pub` so wrapper providers (c411) can
  prime entries from non-search code paths (featured)

### Releasing a new TV APK

The TV APK is self-hosted at `https://uplg.xyz/app-release.apk` and
the in-app updater (`AppUpdater`) pulls it on demand. Settings → App
update also displays "Update available: X.Y.Z" when the installed
version is stale — that check reads a sidecar text file.

**Every time you upload a new `app-release.apk`, also upload
`app-release.version`** to the same directory. The sidecar is a
plain-text file containing only the semver of the APK on one line
(no leading `v`, no whitespace beyond a trailing newline):

```
0.2.0
```

`AppUpdater.fetchLatestVersion()` GETs it, validates with the
semver regex, and the Settings card compares against
`BuildConfig.VERSION_NAME`. Missing / unreachable sidecar = the
card shows "Latest available · checking…" indefinitely but the
Download button still works (best-effort; never blocks the update
path).

When the sidecar drifts from the APK (e.g. you forgot to update
it), the worst case is the user gets a "Up to date" message and
needs to manually hit Download. Don't let that drift — set up the
two uploads in the same script.

### Versioning & client gate

**All three components** carry the same semver-aligned version
(workspace `Cargo.toml`, `web/package.json`, `android-tv/app/build.gradle.kts`
`versionName`). The Android `versionCode` is a monotonic integer
incremented alongside every `versionName` bump.

**`X-Iris-Client: <kind>/<semver>` header** is stamped by every Iris
client on every request:

- Android TV: `studio.kahn.iris.tv.data.HttpClient` interceptor, value
  built from `BuildConfig.VERSION_NAME`.
- Web: `web/src/lib/api.ts` `clientHeaders()`, value baked at build
  time from `package.json` via the Vite `define` (`__IRIS_WEB_VERSION__`).

The backend middleware
(`crates/iris-api/src/client_version.rs::client_version_layer`):

- **Logs** parsed `(kind, version, path)` via `tracing::debug!` — that's
  the install-base / staleness telemetry source. Use these logs to
  decide whether bumping `MIN_TV_VERSION` is safe.
- **Gates** when `client.version < MIN_*_VERSION`: returns `426 Upgrade
Required` with `{"error":"client_outdated","message":...,"min_version":...}`.
- Legacy clients with **no header** are let through (we never break
  shipped APKs retroactively).

**When to bump `MIN_TV_VERSION` / `MIN_WEB_VERSION`** in
`client_version.rs`:

- ONLY when a breaking API change is unavoidable and additive paths
  (new fields, new endpoints, optional enum variants) have been ruled
  out.
- Wait ≥ one full APK release cycle after the new client ships so most
  users have a clean update path.
- The TV update flow goes through `AppUpdater` (downloads APK from
  `https://uplg.xyz/app-release.apk`, **bypasses the Iris backend**),
  so even a fully-gated user can still update from Settings → Update
  app. The 426 overlay (`IrisRoot::ClientOutdatedOverlay`) hides
  whenever the user navigates to the Settings route.
- Web update flow is "reload the page" — the bundle is served by the
  backend, so a redeploy + reload pulls the new bundle.

### Backward compatibility (API evolution)

Iris ships **APKs to real users**. Backend deploys must never crash an
older client. Discipline:

**Adding a field to a response struct** — safe by default:

- Web: TypeScript doesn't validate at runtime; unknown fields are
  inert. Mark new TS fields optional (`description_format?: …`) with
  a default fallback at the call site.
- Android TV: `AppContainer.kt` Json is configured with
  `ignoreUnknownKeys = true` — unknown fields are silently dropped.
- Backend: when adding a field whose old payloads won't carry, use
  `#[serde(default)]` so missing values deserialise to the type's
  default. (`TorrentDetails::description_format` is the canonical
  example — defaults to `Bbcode` for legacy torr9.)

**Adding an enum variant** — needs forward-compat config:

- Kotlin enums **throw** on unknown variants by default.
  `AppContainer.kt` has `coerceInputValues = true` so unknown values
  fall back to the Kotlin default. Every enum data class must declare a
  default (e.g. `descriptionFormat: DescriptionFormat = …BBCODE`).
- TypeScript: new variants widen the union — call sites with
  exhaustive `switch` need a `default:` branch.

**Removing or renaming a field** — never do it directly:

- Treat as breaking. Either keep the old field around (deprecated) for
  one APK release cycle, or version the endpoint (`/api/v2/…`).

**Changing a field's type** — also breaking. Add a new field, migrate
clients, then delete the old one across cycles.

Before merging a change to a serialised type (Rust `Serialize`/`Deserialize`
in `iris-core` or the `routes/` JSON shapes), check: does the running
APK still parse this? If unsure, deploy to a staging instance, hit it
with the prod APK build, and confirm no `JsonDecodingException` in
logcat.

### TMDB resolution

Indexer-provided `tmdb_id`s are unreliable. The pipeline:

1. On every result, parse SCENE filename → title + year + season/ep
2. Hit TMDB `/search/multi`, verify match
3. Backfill the verified id into `torrents.tmdb_id` (migration 0007)

See [memory `project_tmdb_resolution`] for the why and edge cases.

### TorrentSource

```rust
enum TorrentSource { Magnet(String), TorrentFile(Vec<u8>) }
```

`.torrent` file content is bencoded — first byte is `b'd'`. Providers
that return non-bencoded bodies trigger a `Provider` error rather than
silently feeding garbage to librqbit.

### Config

- `config.toml` (server/storage/auth/tmdb) loaded via figment
- `providers.toml` loaded separately (path in `config.toml`)
- Env overrides: `IRIS_<SECTION>__<KEY>` (double underscore = nesting)
- Secrets never inline — use `<key>_env = "ENV_VAR_NAME"` pattern
  (`field_or_env` helper in `crates/iris-providers/src/util.rs`)

### Discriminated unions across stacks

Rust `#[serde(tag = "X")]` requires `@JsonClassDiscriminator("X")` on the
Kotlin sealed class side. See memory `project_serde_kotlinx_discriminator`.

## UI rules

- **English-only visible strings** (memory `feedback_ui_language`) even
  when chatting/coding in French.
- shadcn components: add via `bun x shadcn@latest add <component>` from
  `web/`. Don't hand-author into `src/components/ui/`.
- Tailwind 4: use `@import "tailwindcss"` syntax in CSS; no
  `tailwind.config.js`.
- React 19 + react-router 7 + TanStack Query 5 — all major versions
  current; check breaking-change notes before backporting old patterns.

## Things to avoid

- Don't `cd <project>` to run cargo — already operates on the workspace.
- Don't use `cargo test ...` raw if rtk's hook hides output; use
  `rtk proxy cargo test ...` for unfiltered stdout when debugging.
- Don't add a per-provider feature flag in `iris-providers` — gate
  enablement at config level (`enabled = false`), keep one compilation
  unit so the registry stays uniform.
- Don't introduce a new HTTP client in providers — reuse `reqwest` with
  the workspace defaults (rustls, gzip, brotli, http2 already enabled).
- Don't hand-roll Torznab category lists — defaults in `torznab.rs`
  follow the spec's `2xxx`/`5xxx` buckets; override per-provider only
  when an indexer uses non-standard codes.

## Where to look

- Deployment / Cloudflare tunnel: `docs/DEPLOYMENT.md`
- Database schema: `migrations/*.sql` in numeric order
