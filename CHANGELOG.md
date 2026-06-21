# Changelog

All notable changes to Iris are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.1] - 2026-06-21

### Added

- **Nexum indexer** — a new tracker consumed through the generic `torznab`
  kind (no bespoke module). Drop `NEXUM_API_KEY` into `.env` to enable it;
  until then the registry logs one line and skips it (never blocks boot).
- **Android TV — "Back to Home" button in the player.** A focusable control in
  the ExoPlayer controller header wired to the nav-up callback, so the remote's
  D-pad reaches it without leaving the video surface.

### Fixed

- **Sessions no longer drop under concurrent token refreshes.** When several
  clients refresh near-simultaneously (multiple tabs sharing a cookie jar, a
  request retried after a network blip) the first rotation revoked the token and
  the stragglers got a 401, logging the user out of a live session — aggravated
  by the household sharing one Cloudflare/NAT IP. Fixes: a refresh-token
  **rotation grace** window (migration 0030 — `rotated_at` distinguishes a
  rotated token from an explicitly revoked one, and a straggler within the
  window is re-issued instead of rejected); the web client refreshes
  **single-flight** and only a genuine 401/403 clears the session; auth cookies
  carry `Max-Age` so a session survives a browser restart; the rate limiter
  splits auth refreshes into their own lane so they aren't throttled alongside
  logins; Android TV pairing polls with backoff.
- **Anime shows splitting into two identical-looking collections (with episodes
  going missing).** A series that ships under both a fansub group (`-Tsundere-Raws`
  → flagged anime) and a scene/Seedpool group (`-MonoDiSC` → not) used to land in
  two collections with the *same* display title, and a new episode only ever
  attached to the half matching its release group — so it appeared "nowhere" on
  the half the user was watching. When an `anime:K` and a plain `K` collection
  resolve to the **same** TMDB id they now auto-merge into one (the legitimate
  same-title split — anime vs live-action *One Piece*, which carry *different*
  ids — is preserved). The episode scheduler now collects **both** naming styles
  for a collection that's alone on its title, so cross-convention releases stop
  being dropped. Self-heals on boot and every 5 minutes; no manual fix needed.

## [1.0.0] - 2026-06-20

### Added

- **Content-first recommendation engine** (new `iris-reco` crate). Lightweight,
  CPU-only `model2vec` embeddings plus a per-user multi-centroid taste profile
  rank the catalogue by cosine similarity, surfacing relevant titles **beyond the
  rolling discovery window** — not just the freshest uploads. The model runs only
  on the ingest path; requests rank over cached vectors. New shelves: "Picked for
  you", "Popular in your circle", "Fresh drops", each card carrying a short reason.
  (migrations 0027 / 0029)
- **"Tonight" genre/mood browse** (web **and** Android TV). The board is the live
  TMDB genre taxonomy ordered by the user's taste (their genres first); picking
  one returns a recency-filtered, taste-ranked, grabbable selection (catalogue ∪
  broad TMDB discover), with clean slug URLs (`?mood=horror`).
- Out-of-window watched-title backfill (with AniList correlation for anime) so a
  user's full taste profile is embedded.

### Changed

- **Unified TMDB identity on `collection.tmdb_id`** — `torrent.tmdb_id` is no
  longer written anywhere; every read and API facade resolves through the
  collection's single verified id. One logic, no duplication.
- Recommendation candidates no longer propose an aberrant release size for their
  kind (e.g. a 4K REMUX movie); an over-cap row degrades to a re-searchable
  candidate so the grab picks a saner release. Configurable via
  `[reco] max_movie_gib` / `max_tv_gib`.
- Navigation order: Home › Search › Tonight › For You › Library › Admin (web);
  Search moved first on Android TV.

### Fixed

- Reco "Because you watched …" could name an unrelated title when a movie and a
  TV show shared a numeric TMDB id — every recommendation join is now kind-aware.
- Anime / live-action collection splits self-heal (migration 0028).

## [0.9.1] - 2026-06-18

### Fixed

- Library posters could resolve to an unrelated title (e.g. _Supernatural
  (2005)_ showed another show's art, _Midnight (2021)_ showed a different
  film). TMDB resolution now keys on the collection's SCENE identity
  (`display_title`) instead of the raw torrent name, and uses TMDB's typed,
  year-filtered search (`/search/movie` + `primary_release_year`,
  `/search/tv` + `first_air_date_year`) so a common title isn't drowned by
  the popularity-sorted multi-search. Existing collections self-heal on boot.
- Android TV launcher-channel posters could show an unrelated image when a
  movie and a TV entry shared a numeric TMDB id — every lookup now passes the
  media kind to disambiguate the namespaces.

### Changed

- Unified poster sourcing across every library surface — the home shelf, the
  library grid, the collection page and the per-torrent views all derive their
  poster from the parent collection's resolved TMDB id (web **and** TV),
  instead of a per-torrent ingest-time hint. The torrent list now serves
  `COALESCE(collection.tmdb_id, torrent.tmdb_id)` so the convergence needs no
  client change.
- Quieter production logs: the collection-identity heal "target key already
  owned" skip and the TMDB "no match" line dropped to `debug` (both re-fired
  every scheduler tick with nothing to act on).

## [0.9.0] - 2026-06-18

### Added

- Android TV now consumes the backend **OpenAPI contract**: the whole DTO layer
  is generated from `openapi.json` (kotlinx-serialization), on par with the web
  client — hand-maintained models can no longer drift from the server.
- CI gate (GitHub Actions): `clippy` + `rustfmt --check` + `cargo-deny`
  (licenses / advisories / sources). Pushing a version tag builds the release
  APK and publishes a GitHub release from this changelog.
- Supply-chain policy via `cargo-deny` (`deny.toml`).
- `justfile` task runner (replaces the `Makefile`).

### Changed

- Upgraded all Rust, web, and Android dependencies to their latest stable
  releases.
- Android: `compileSdk` / `targetSdk` 37, Kotlin 2.4, Media3 1.10.1 (the native
  ffmpeg / dav1d decoder AARs were rebuilt to match).
- Promoted the OpenAPI tagged-union schemas to proper discriminators so every
  client generates clean sealed types.

## [0.8.1] - 2026-06-15 — Road to 1.x

### Added

- **10-bit AV1 on TV.** Set-top boxes without a hardware AV1 decoder now play
  10-bit AV1: the server re-encodes to H.264 when the TV declares no hardware
  decoder, on top of the full dav1d configuration. Older boxes (pre-2021,
  low-end) are best kept off AV1 10-bit; browsers handle it everywhere.

### Changed

- Updated all backend and frontend dependencies.
- Migrated the entire web app to OpenAPI-generated typings (Android TV to
  follow) to ease backward compatibility going forward.

## [0.8.0] - 2026-06-09

### Added

- Search surfaces what you already own first, episode-aware — no more
  re-downloading a movie that's already in the library.

### Fixed

- Automatic disk cleanup no longer kills an active stream when a video is
  stopped or restarted mid-playback.
- After a brief network hiccup the player resumes exactly where you were
  instead of starting over; watch progress can no longer be lost without a
  deliberate seek.
- Frozen web streams now recover on their own (no more pause/play workaround).

## [0.6.0] - 2026-06-05

### Added

- Recommendation system — still building up, and it grows more relevant the
  more you watch.

### Changed

- Restore each user's preferred language on Web and TV (unless a title was
  already watched in another language, which then stays the default to avoid
  disrupting history).

## [0.5.1] - 2026-05-25 — Edge cases

### Added

- Watch-party basics (#2).

### Changed

- Watched shows move to "Completed" once the user switches to the next one.

### Fixed

- Race that led to `Unauthorized` on TVs after a period of inactivity.
- Play / resume handling in the Collections page on Web.
- Edge case on some MP4 + AC3 files with exotic first samples in the web libav
  decoder.

## [0.5.0] - 2026-05-24

### Changed

- Full Android TV redesign (#1), with new Iris TV branding (clear the icon from
  the home row and re-add it to pick up the new drawable).
- Updated the web favicon.

## [0.4.0] - 2026-05-20

### Added

- Smarter search: parses `S04E11` and ranks the right episode first instead of
  season packs.
- Already-owned results offer "Play existing" instead of triggering a duplicate
  download.
- Per-user Watchlist, auto-populated from what each user grabs (no Follow button
  to wire up).
- Language badges (FR / EN / MULTi) on every search result, episode and pack
  when the signal is available — multiple indexed languages for the same episode
  show as side-by-side chips on the collection page; click yours and Iris grabs
  the matching release.
- Season-pack support: click any missing episode and Iris downloads the pack,
  then resolves your episode inside it.
- TMDB posters and indexer offers pre-warmed on freshly-ingested collections —
  no more empty page on first visit.
- Grab survives server restarts (Torznab / UNIT3D `.torrent` URLs persisted to
  the DB), with hot re-prime via a fresh search if the cache stays cold.
- TV: server-side HLS remux fallback when ExoPlayer chokes on a codec /
  container (DV, Atmos, exotic MKV), with a progress overlay polling
  `/play/status`.

### Changed

- Unified series page `/collection/:id` is now the single surface; `/series/:id`
  retired.
- Search cards show the full title with a compact `S04E11` chip above it.
- TV: significant UX pass — clean D-pad focus, no more out-of-screen scaling,
  readable focused season tabs.

### Fixed

- SCENE-aware file ordering so TV pack file lists appear in episode order.

## [0.3.0] - 2026-05-17 — Refine TV UI / season-pack parsing

### Added

- A tiny "seedbox" interface on TV to manage the library (remove items),
  mirroring the Web UI.

### Fixed

- Crash parsing specific season packs (`S02 E01` → `S02E00`) that took down the
  TV app in the library.

## [0.2.3] - 2026-05-16 — Seedpool + Android TV UI

### Added

- List mode in the Android TV app, plus a marquee on the card-view title.
- Seedpool as a supported source (UNIT3D standard provider).

### Changed

- Search is preserved when opening a release detail and going back.

## [0.2.1] - 2026-05-15 — DTS sound handling

### Changed

- Tier B player now handles DTS sound.
- Android TV bundles the ffmpeg extension to software-decode anything the device
  can't handle in hardware.

## [0.2.0] - 2026-05-14 — No remux + Android TV QoL

### Added

- Torznab-style provider handling (c411 fully supported).
- In-browser decoding that bypasses server-side remuxing — instant start even
  while still downloading.

### Changed

- Robust backward-compatibility handling.
- Web: cursor hides in fullscreen and the volume-slider dot is hidden by default.

### Fixed

- Android TV previous / next episode navigation.

## [0.1.0-alpha5] - 2026-05-14

### Changed

- Selective remuxing — only when actually required. Roughly halves disk usage.

## [0.1.0-alpha] - 2026-05-10

### Added

- Initial prototype line (`alpha` … `alpha4`): the first end-to-end Iris builds.

[Unreleased]: https://github.com/uplg/iris/compare/0.9.1...HEAD
[0.9.1]: https://github.com/uplg/iris/compare/0.9.0...0.9.1
[0.9.0]: https://github.com/uplg/iris/compare/0.8.1...0.9.0
[0.8.1]: https://github.com/uplg/iris/compare/0.8.0...0.8.1
[0.8.0]: https://github.com/uplg/iris/compare/0.6.0...0.8.0
[0.6.0]: https://github.com/uplg/iris/compare/0.5.1...0.6.0
[0.5.1]: https://github.com/uplg/iris/compare/0.5.0...0.5.1
[0.5.0]: https://github.com/uplg/iris/compare/0.4.0...0.5.0
[0.4.0]: https://github.com/uplg/iris/compare/0.3.0...0.4.0
[0.3.0]: https://github.com/uplg/iris/compare/0.2.3...0.3.0
[0.2.3]: https://github.com/uplg/iris/compare/0.2.1...0.2.3
[0.2.1]: https://github.com/uplg/iris/compare/0.2.0...0.2.1
[0.2.0]: https://github.com/uplg/iris/compare/0.1.0-alpha5...0.2.0
[0.1.0-alpha5]: https://github.com/uplg/iris/compare/0.1.0-alpha...0.1.0-alpha5
[0.1.0-alpha]: https://github.com/uplg/iris/releases/tag/0.1.0-alpha
