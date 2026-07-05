# Changelog

All notable changes to Iris are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.2.0] - 2026-07-06

### Added

- **Live TV** (web + Android TV). A new "Live TV" section streams free
  over-the-air channels — France's TNT curated and pinned in Arcom order,
  then the rest of the country's catalogue by category, and **any other
  country** via a picker. Channels come from the community
  [iptv-org](https://iptv-org.github.io/) database. Everything plays through
  a backend HLS proxy: the upstreams are plain-HTTP, CORS-less and often
  demand a browser `User-Agent`, so clients never touch them directly; proxy
  URLs are HMAC-signed so the endpoint can't be turned into an open proxy.
  - **Now / Next guide.** Each channel shows the current and upcoming
    programme with a progress bar, from a nightly XMLTV feed.
  - **Stability-first fallback.** iptv-org lists several feeds per channel;
    Iris probes them, never elects a feed that doesn't respond, escalates a
    dead feed's cooldown, and rotates to the next candidate when the one
    you're watching dies (a "Try another source" control is there too). The
    player itself reports unplayable feeds so a bad source is demoted for
    everyone.
  - **E-AC-3 audio in the browser** — handled client-side by the same
    decoder path the VOD player uses, no server-side transcoding.
  - **Adaptive channel logos.** Logos are proxied (killing hotlink/CORS
    noise) and sit on a plate whose shade is picked from the logo's own
    luminance, so a black-on-black or white-on-white logo stays legible.
- **Continue Watching now moves with you.**
  - Finish an episode (pass 90 %) and the shelf automatically advances to the
    **next episode** of that season — even one you haven't started yet —
    instead of the show dropping off.
  - **Manage a tile**: web shows a hover menu, Android TV a long-press menu,
    each with **Mark as watched** and **Remove from Continue Watching**.
    Removing a series hides it until you play a newer episode (it doesn't
    silently come back), Netflix-style.

### Changed

- **A title counts as "watched" at 90 %** of its runtime, not the last 30
  seconds. Movies leave Continue Watching once you pass 90 % and stop; an
  episode counts as finished at 90 % so the shelf can advance. Applied
  identically on web and Android TV.
- **`quick-xml` upgraded to 0.41** to clear two upstream security advisories
  (RUSTSEC-2026-0194 / -0195 — a quadratic-parse and an unbounded-allocation
  DoS in XML parsing).

### Fixed

- **Anime "Specials" were mis-tagged as regular episodes**, producing a bogus
  season/episode and polluting the release list; the SCENE filename parser
  now recognises the Specials bucket.
- **The "currently watching" surface listed duplicates / the wrong entry** in
  some multi-release cases; episodes are now de-duplicated on a stable
  identity.
- **Season-pack suggestions no longer fight what you already own** — the
  available-episodes/library logic was tightened so already-covered seasons
  and languages aren't re-offered.
- **Android TV history UI** polish (navigation and layout fixes).
- **Android TV player — removed the top-right "‹ Prev / Next ›" episode
  chips.** They lived in a Compose overlay above the native player, outside
  its focus hierarchy, so the D-pad could never reach them. Next-episode
  navigation is the native control-bar button (which *is* reachable);
  previous-episode selection lives on the series screen.

## [1.1.0] - 2026-06-30

### Added

- **Watch History** (web + Android TV): a per-episode log of everything
  watched, in-progress and completed — including episodes whose source
  torrent has since been deleted (disk-reclaim GC, admin cleanup), which
  used to just vanish from "Continue watching" with no way to find "where
  was I". Virtualized list on both clients; reachable from the web's
  primary navigation and from TV Settings. Admins can drill into any
  household member's history from the Users list on `/admin`.
- **Admin audit log**: a persisted, queryable record of sensitive actions
  (torrent deletions, password resets, display-name changes,
  admin-triggered GC, remux-cache wipes) — replaces the previous ephemeral
  `tracing::` logs, which rotated out and weren't visible anywhere.
  Virtualized list on the admin page.
- **Web — grab the next episode straight from the player.** The watch
  page's side panel now also lists episodes Iris has discovered but not
  downloaded yet ("S01E17 available"); a single "Grab & Play" click grabs
  it and jumps straight to playback.
- **Android TV — software-decode awareness on search results.** Release
  titles are now parsed for a coarse codec hint (H.264 / HEVC / AV1 /
  VP9); the TV badges and deprioritises (never hides) AV1 / VP9 results
  the connected box can't hardware-decode.
- **Season-pack suggestions no longer redundant with what's already
  owned.** If a `MULTi` episode release already exists for a season, FR
  and EN season-pack offers are hidden; if a French episode release
  exists, `MULTi` and another French pack are hidden. English packs are
  never suppressed by French coverage alone.

### Fixed

- **Android TV — season selection reset to Season 1 after Back from the
  player.** Browsing Season 2, playing an episode, then pressing Back
  used to land back on Season 1 — the selection was held in `remember`
  instead of `rememberSaveable` and didn't survive the back-stack restore.
- **Android TV — D-Pad Up from Prepare/Play skipped the season tabs.**
  Compose's default spatial focus search picked the hero's Back button as
  the nearest target across the nested season-tabs/episode-list layout;
  Up now explicitly lands on the active season pill.
- **Android TV — playback kept streaming after leaving via Home.** No
  lifecycle observer existed anywhere in the player path, so the torrent
  stream and the progress heartbeat kept running in the background. The
  player now pauses (and the admin "Now watching" view reflects it) on
  `ON_STOP`.
- **Android TV — the "Next episode" prompt was unreachable by D-Pad.** It
  lived in a Compose overlay on top of the native `PlayerView`, outside
  its focus hierarchy. It's now a native control-bar button, reachable
  like every other transport control.

## [1.0.2] - 2026-06-21

### Added

- **Native Nexum provider** (`kind = "nexum"`), replacing the generic Torznab
  bridge for Nexum. It talks to Nexum's REST API directly, which exposes the
  real per-torrent category — so games / software / audio / ebooks are filtered
  out instead of leaking into search as fake "Movies" (Nexum's Torznab bridge
  collapsed every non-video category into `2000`). Also brings stable numeric
  torrent ids, BBCode descriptions and `tmdb_id`.

### Fixed

- **Android TV search crash on multi-result Nexum / Torznab queries.**
  quick-xml 0.40 splits an element's text at every `&` entity, so a `<guid>`
  download URL (`…?t=download&id=…&apikey=…`) was reduced to just the shared
  apikey — handing every result the SAME `external_id`. The TV keys its result
  grid on `provider_id:external_id`, and a duplicate key throws in Compose, so
  the app crashed (only on searches returning ≥2 such results). The Torznab
  parser now joins entity-split fragments and pulls the unique torrent id; this
  also fixed `resolve()` grabbing the wrong torrent (the link cache shares that
  id) and any title / link containing `&`.
- **Search relevance ("Recommended") now favours tighter title matches.** The
  old exact/substring buckets dropped every loose match into one tier, then
  size + seeders decided order — so a release merely *containing* a query word
  could outrank the real match. Scoring is now graded by query-token coverage
  with a padding penalty, so the closest title wins on relevance before
  popularity is consulted.
- **Large (>1080p) H.264 on a 1080p-capped device now plays via a server
  downscale instead of looping.** A 1440p / 4K H.264 file the device's hardware
  decoder rejects (e.g. a Chromecast HD, capped at 1080p) used to be
  stream-copied unchanged on the Tier-F remux — i.e. handed back the exact frame
  size it just refused → an endless "remux" loop. The server now re-encodes such
  sources down to a 1080p H.264 baseline.
- **Android TV — that downscale no longer hangs on "loading".** The downscale
  streams as a growing HLS playlist; the fallback prepared the player against a
  cold cache and timed out, leaving it stuck until the user backed out and
  re-entered. The player now waits for the server build to pass the resume point
  before its single `prepare()` (as the proactive transcode path already did),
  so it starts on a warm cache and streams while the encoder runs ahead — never
  waiting for the full encode.
- **Android TV — subtitles on the server remux/transcode path.** That path's
  HLS master playlist carries no subtitle renditions (subs stay external), so
  it showed none. Text subtitle tracks are now side-loaded as WebVTT from the
  per-track route; the raw `/stream` path is byte-identical to before (Media3
  still reads its container subs — incl. PGS — natively).
- **Anime collections — duplicate split now merged at ingest.** When two
  releases of one anime classify differently (a `[Fansub]` release →
  `anime:title`, a scene-named one → `title`) they land in separate
  collections. The safe twin-merge — gated on both halves resolving to the same
  TMDB id, so the legitimate One Piece anime-vs-live-action split is never
  collapsed — now also runs at ingest instead of only on the 5-minute backfill
  sweep, closing the window where a duplicate is briefly visible.
- **Torznab CDATA titles.** Indexers that wrap `<title>` (and occasionally
  `<link>` / `<guid>`) in CDATA no longer render with empty titles — CDATA is
  routed through the same field dispatch as plain text.

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

[Unreleased]: https://github.com/uplg/iris/compare/1.2.0...HEAD
[1.2.0]: https://github.com/uplg/iris/compare/1.1.0...1.2.0
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
