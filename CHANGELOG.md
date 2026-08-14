# Changelog

All notable changes to Iris are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.4.0] - 2026-08-10

### Added

- **Admins can delete an account.** `DELETE /api/admin/users/{id}` plus a
  destructive confirm dialog on the admin Users card. Everything personal
  (sessions, watch progress, follows, preferences, dismissals) cascades
  away; the target's grabs are re-attributed to the acting admin instead
  of cascading out of the shared library (`torrents.added_by` is
  `ON DELETE CASCADE`). Deleting your own account is refused, which
  doubles as the "at least one admin remains" guarantee. Migration 0036
  rebuilds `audit_log` without its FK on `actor_id`: the 0031 comment
  already promised the audit trail outlives the account, but the
  constraint blocked deleting any user who had ever produced an audit
  row — reads now LEFT JOIN users with a "deleted user" fallback, and
  the deletion itself is audited (`user.delete`, target email in the
  details).
- **Grabbing a release over 50 GB now takes an explicit second
  confirmation.** Born from a complete-series pack grabbed right after
  the one wanted season: huge packs hog the shared disk and get
  everyone's library GC-evicted sooner. Web: the first Play click in the
  preview dialog arms a loud warning banner and swaps the CTA for a
  destructive "Yes, download X"; chains cleanly with the
  duplicate-in-library confirm. TV: same guard through the existing
  ConfirmDialog on the search detail screen.
- **YggReborn provider** (francophone) — pure config on the generic
  Torznab provider: clean Torznab 1.3 (standard 2000/5000 buckets,
  `guid` = bare infohash so external ids stay stable, token-signed
  links returning real bencoded `.torrent` files). `base_url` points at
  `api.yggreborn.org` directly — the advertised `www` host 302s there
  on every request.
- **Memphis provider**, plus a generic `tvsearch_q = false` Torznab
  option it needed: Memphis's `t=tvsearch` returns 0 hits whenever `q`
  is present (and ignores `season`/`ep`) while `t=search`/`t=movie`
  match fine. The flag routes TV queries through the generic op with
  the raw SxxExx query — episode-level substring matching verified —
  keeping the `cat=5000` filter; the no-query `latest` browse keeps
  `t=tvsearch`, which works. Compliant indexers are untouched.

### Fixed

- **Seed ratios now divide two lifetime quantities.** The global
  "Seeded all-time" ratio divided lifetime upload by the LIVE torrents'
  progress (admin: by current disk usage), overstating it as soon as
  the GC had evicted anything; the per-torrent ratio divided lifetime
  upload by the current session's progress, so a regrab inherited its
  previous life's upload and showed an instant bogus ratio. Migration
  0037 adds `torrents.downloaded_bytes_total` (max on-disk progress
  ever observed, reconciled by the 30 s seed-stats tick — progress is
  absolute state, so max-ever is the honest approximation and restarts
  can't phantom-count the recheck climb), backfilled from
  `total_size_bytes` for every finished row including evicted ones.
  Web (library summary + per-torrent rows + admin storage) and TV
  (TorrentsScreen summary + rows) all divide lifetime by lifetime.

- **Native (SRT/WebVTT) subtitles no longer stay blank when resuming
  mid-file on a still-downloading torrent.** The `.vtt` extracted from
  a partial source is truncated at the first sparse hole, and a
  `<track>` element fetches its src exactly once — so a resume past the
  hole had no cues for the whole session (toggling the track only flips
  `mode`, which never re-fetches; only restarting from 0 showed the
  early cues). The `?v=<subtitleVersion>` catch-up mechanism the
  ASS/PGS overlay path already had now also repoints the native
  `<track>` src on every progress milestone, making the browser re-run
  the track fetch in place across Tier A/B/F.

### Changed

- **Frontend dependency pass — everything to latest stable.** Notables:
  mediabunny 1.50.8 → 1.53.1 (the DTS + open-GOP patch re-verified
  against upstream — neither landed there — and regenerated as
  `patches/mediabunny@1.53.1.patch`; the two `AudioSampleSource` call
  sites migrated off the now-deprecated `bitrate` field to
  `quality: new Quality({ bitrate })` — the bare-number `Quality` form
  means a 0..1 qualitative level, not a bitrate), hls.js 1.6.16 → 1.7.0
  (no removed exports; the new `liveMaxUnchangedPlaylistRefresh` live
  error defaults to `Infinity`, so the tuner path is unaffected), plus
  React 19.2.8, Vite 8.2.1, TanStack Router/Query/Virtual, Tailwind
  4.3.3, radix-ui, lucide-react, shadcn CLI, oxlint 1.78, oxfmt 0.63
  (no reformats) and friends. `tools/api-gen` is already current and
  deliberately stays on TypeScript 6 (needs `ts.factory`).

## [1.3.6] - 2026-08-09

### Added

- **The TV APK now works on regular Android phones.** It always installed
  there (leanback/touchscreen deliberately not required), but every
  control ignored taps: tv-material's internal clickable modifier is
  focus + D-pad key events only, with no pointer handling at all
  (verified against the tv-material 1.1.0 bytecode). A `touchClick`
  modifier (tap + long-press via `pointerInput`) now rides alongside
  every clickable Surface/Card/Button — centralised in the IrisButton /
  TvIconButton / ConfirmDialog wrappers and swept across the screens'
  direct call sites. D-pad behaviour and TV visuals are untouched; the
  activity is pinned to `sensorLandscape` since every layout is
  10-foot landscape. Playback was already touch-capable (Media3
  `PlayerView` handles its own pointer input), and lazy lists scroll by
  touch natively. Known limits, accepted: no ripple/focus highlight on
  tap, and the UI stays TV-scaled on a 6-inch screen — the web app
  remains the first-class phone experience.
- **Phone-mode follow-ups from the field tests.** Window insets are now
  handled explicitly (`enableEdgeToEdge` + one `safeDrawing` pad at the
  app root — all-zero on TV): no more buttons under the status bar in
  portrait, and the soft keyboard resizes the content instead of
  covering the sign-in inputs (`adjustResize` is ignored under
  edge-to-edge, which is enforced by targetSdk 35+ anyway). The sign-in
  form also scrolls. Search and Discover grew a visible Back button
  (every other screen already had one; on a phone the only exit was a
  system gesture). The home hero is sized explicitly per orientation —
  landscape/TV keeps the 78% billboard with a floor so the Resume
  button can't be crushed (a `heightIn` floor chained onto
  `fillParentMaxHeight` does NOT coerce — field-tested), portrait gets
  a capped 45% billboard with a full-width lockup instead of the
  stretched-void look. Browsing rotates freely; the two playback
  screens pin sensor-landscape AND go immersive (system bars hidden,
  swipe to peek, drawing into the display cutout à la Netflix), which
  also collapses the root safe-zone padding so video is genuinely
  full-bleed — the root pads `systemBars ∪ ime` rather than
  `safeDrawing`, whose never-zero cutout inset kept a notch-sized dead
  band on the watch screen. `keepScreenOn` was already set on both
  player views, so playback never sleeps the phone.
- **Search header redesigned — one layout for TV and phone.** The old
  header (420dp field + Search + Mic buttons + three labelled chip
  groups spread over two rows with a stretching spacer) overflowed
  portrait phones, read as giant voids on landscape phones, and
  truncated "Series" on TV. Now: `[back | full-width input with the mic
inside]` over `[type icons (all/movie/series) | one cycling sort pill
| one grid/list toggle]` — 17 interactive stops down to 7, no group
  labels, no per-form-factor fork. The IME action is the single submit
  path; initial D-pad focus lands on the input (the new Back button had
  stolen it), and post-submit focus parks on the sort pill so the
  leanback IME detaches.

## [1.3.5] - 2026-08-09

### Added

- **RAR'd releases are refused at grab time (all providers).** Scene
  releases packed in `.rar`/`.rXX`/split volumes can be seeded but never
  streamed, so they no longer enter the engine at all: every grab path
  (search ingest, For-You preview, follows auto-grab) parses the
  `.torrent` first and answers `409 archive_only` when the archive bytes
  outweigh the video bytes (a lone `sample.mkv` doesn't count as video).
  The web preview dialog flags the release and disables Play up front;
  `TorrentPreview` gains per-file `is_archive` + top-level `streamable`.
  Byte-weighted on purpose: unrar'd season packs and movies with an
  incidental extras archive still pass.
- **Duplicate-movie guard on ingest.** Grabbing a movie whose collection
  already holds a live copy now answers `409 duplicate_in_library`
  (message lists the existing releases) unless the client passes the new
  `allow_duplicate` consent flag. Web preview dialog and TV
  search-detail/voice-pick flows surface a "Download another copy?"
  confirmation; explicit re-grabs (history restore, ghost-resume,
  language variants, `/regrab`) skip the guard by design. TV grabs also
  parse the error envelope now, so guard messages read as intended
  instead of `HTTP 409`.
- **Multi-copy movies are visible and manageable.** The web collection
  page no longer auto-plays `torrents[0]` when a movie has several
  copies: it stops on a per-release list (name, size, who added it,
  when) with Play on the release's main file and a two-step delete. The
  TV collection screen groups the movie file fallback under one header
  row per release with the same delete (ConfirmDialog +
  `DELETE /api/torrents/{infohash}`).
- **TorrentLeech provider** (`kind = "torrentleech"`, id `tl`). Search rides
  the site's JSON browse endpoint behind a cookie session (site
  username/password, optional 2FA token), downloads ride the per-user RSS
  key — signed, cookie-less URLs that survive process restarts and session
  expiry. Coverage is every video category (movies Cam→4K, DVD, boxsets,
  documentaries, foreign; TV episodes SD/HD, boxsets, anime, cartoons,
  foreign); games/music/ebooks/apps are filtered out at query time. The
  query-less "recent torrents" view feeds the discovery catalogue's
  `latest()` poll. Needs `TL_USERNAME`, `TL_PASSWORD`, `TL_RSS_KEY`.

### Fixed

- **UNIT3D auth also rides `Authorization: Bearer`.** The token used to
  travel only as the `?api_token=` query param; newer UNIT3D
  deployments reject that form (it leaks tokens into server logs). The
  header is now sent on every request alongside the query param, which
  older instances (Seedpool) simply ignore.

- **UNIT3D `resolve()` recovers from stale download links.** The
  pre-signed `download_link` embeds the user's rsskey; when the indexer
  rejects it (UNIT3D 302s the dead link to the torrent's web page,
  which our JSON `Accept` turns into `401 Unauthenticated` — the TOS
  preview failure), every cached and DB-persisted link dies at once. A
  failed cached-link download now falls through to
  `/api/torrents/{id}`, which ships a freshly-signed link, re-primes
  the cache, and retries once — also fixing the long-standing "no
  download URL cached" cold-cache error after a restart.

## [1.3.4] - 2026-07-26

### Fixed

- **Continue Watching can no longer send you to a dead player page.** Tiles
  whose file the engine can't serve anymore (GC-reclaimed, lost by a session
  restore, or a grab that died half-way) used to navigate straight to a 404.
  The server now converts them into "grab" tiles — same series, same episode —
  so both web and TV run their existing grab & play flow instead; tiles with
  no episode identity to grab by (movies, unparsed files) are dropped rather
  than served dead. Behaviour is identical on web and TV by construction: the
  conversion happens in the one shared `/api/me/continue-watching` route.
- **Web home actually runs the grab & play flow now.** The discovery-first
  home redesign rebuilt the Continue Watching shelf (and the resume hero)
  with its own cards and never ported the grabbable-tile handling — clicking
  an "Up next" tile with no file on disk navigated to the dead `/watch//0`
  URL. The hero and the shelf cards now grab-then-play like the TV does
  (falling back to the series page when no release is found), grabbable
  tiles are labelled "Up next · SxxExx · Not downloaded", and the orphaned
  pre-redesign `ContinueWatching` component is gone. Also fixes duplicate
  React keys when several grabbable tiles (all sharing an empty infohash)
  were on the shelf.

### Added

- **"Grab it again" on the dead player page (web + TV).** Opening a watch
  link whose release was reclaimed now offers a one-click re-grab via the new
  `POST /api/torrents/{infohash}/regrab` route, which re-ingests the release
  from its recorded provenance — same infohash, so playback resumes where it
  left off. Web also gets a branded Not Found page (router-level 404 and the
  watch page's error states) instead of the bare-bones fallbacks.
- **HD-Torrents provider (`kind = "hdtorrents"`).** New English tracker,
  and the first scraper-based provider: the site has no API at all, so the
  provider logs in with the account credentials (cookie session, automatic
  re-login when it expires) and parses the `torrents.php` HTML — selectors
  validated against live pages, not just the Prowlarr definition the site
  has since drifted from. Searches are scoped to the Movie/TV buckets
  (music/XXX excluded), releases default to English like Seedpool, and the
  site keys everything on the torrent's infohash, which the provider
  surfaces so "In library" matching works. The preview dialog is fed by a
  scraped `details.php` view (IMDb synopsis block, Technical Info blob as
  NFO, genres, file count, live peer counts, 60 s cache) that also primes
  the download-link cache, so grabbing straight from a preview works even
  after a restart. Pulls in `scraper` 0.27 and reqwest's `cookies`/`form`
  features.

### Changed

- **Aggregated search no longer waits for the slowest tracker.** Each
  provider now gets a hard 8 s budget inside `search_all`; one sick indexer
  sitting on the request (looking at you, nginx-that-502s-eventually) used
  to hold the whole `/api/search` response hostage for its full 15–20 s
  client timeout while the healthy providers had answered in milliseconds.
  Stragglers degrade to the existing per-provider error entry.
- **The Torznab layer retries transient 5xx once.** tr4ker's nginx 502s
  briefly when a PHP worker recycles; a single immediate retry makes most
  of those invisible. 4xx stays fatal — a retry can't fix a bad API key.
- **Toolchain bumped to Rust 1.97** (Docker build image `rust:1.97-trixie`,
  workspace `rust-version`).

## [1.3.3] - 2026-07-25

### Fixed

- **Android TV: audio tracks wrongly flagged "forced" are now listed in the
  selector.** Some multi-language MKVs (Breaking Bad S01/S02 notably) carry a
  bogus `forced` disposition on the dub track — a mux error with no real
  meaning for audio. Media3's native settings menu unconditionally hides
  forced tracks, so the track could play via automatic language selection yet
  never appeared in the audio list: once a user switched to another language
  there was no way back. The TV player now strips the forced flag from audio
  tracks at the extractor (subtitles keep theirs — there it drives the
  "Forced" label and auto-show semantics), so every probed language is
  selectable, matching the web player.

## [1.3.2] - 2026-07-14

### Added

- **The next episode is one click away — even before it's downloaded.**
  When you finish an episode and the next one has aired but isn't on disk
  (never grabbed, or reclaimed by the disk cleaner), Continue Watching
  now offers it anyway: "Up next · S08E08 · Not downloaded". One click
  grabs it — in the language the series is already in — and playback
  starts while the download completes (web + Android TV; on the TV the
  hero button becomes "Grab & play"). If no release can be found, you
  land on the series page with every option laid out.
- Continue Watching tiles are labelled with their real "S08E08" from the
  library's episode mapping — no more squinting at release names.

### Fixed

- **Continue Watching follows YOUR watch order.** "Up next" only ever
  suggests the episode right after the one you finished (or the next
  season's opener once you've actually watched the finale — confirmed
  against TMDB). It used to suggest the closest episode _on disk_, so
  finishing S08E07 while someone had downloaded S09E01 skipped you a
  whole season ahead. A mid-season gap now shows nothing rather than
  the wrong episode.
- **AV1 finally uses the TV's hardware decoder.** Decoder choice is now
  per-stream: 8-bit AV1 plays on any AV1 silicon, 10-bit only on
  decoders that declare 10-bit support, and the bundled software
  decoder (dav1d) drops back to being the fallback it was meant to be.
  Boxes with real AV1 hardware but weak CPUs — the Chromecast HD —
  no longer drop frames in action scenes or flicker on restart from
  being forced through software decode. 8-bit-only decoders keep the
  server's 10-bit catch-up transcode (the `Iris-Caps` header only
  advertises `av1-hw` when 10-bit is genuinely supported).

### Changed

- **Live TV picks the most stable source, not just the first.** Channels
  already aggregate every matching feed across playlists as ordered
  fallbacks; now each feed carries a reliability tier derived from its host
  (official broadcaster CDN > ISP restream > community aggregator), and the
  election orders sources by `(tier, quality)` — a healthy official 720p
  outranks a community 1080p. FR / US / IE ship extra curated playlists by
  default (FR: schumijo + Free-TV, official CDNs + a Swiss-ISP TNT restream;
  US: Free-TV, licensed FAST providers Pluto/Tubi/Amagi/Publica; IE:
  Free-TV), so there's real cross-provider redundancy behind every channel.
  Fixed the election reset that made the
  first viewer after a 6 h playlist refresh pay a rotation onto a dead
  feed: the elected source is now seeded from persisted per-URL health
  (a just-dead feed stays skipped instead of being re-tried every refresh).

## [1.3.1] - 2026-07-12

### Added

- **Ghost collections now look like they did before the cleanup.** A show
  or movie whose files were reclaimed keeps its full collection page: the
  same episode list, your "watched" badges and progress intact, with a
  compact "Re-grab" chip in place of Play (web + Android TV). Re-grabbing
  restores the exact same release, so playback resumes right where you
  left off. Works for season packs too: every episode of the pack keeps
  its own watched state. Gone rows are scoped to each user's own history
  (a release you never watched never sprouts a "Re-grab" for you), and
  same-language indexer offers are hidden while their gone twin is shown.
- **Hide a Gone entry.** Each user can now dismiss what they don't want to
  see anymore, without touching anyone's watch history:
  - a single reclaimed release, from the collection page (web: the ×
    button; TV: long-press the chip, or the Hide button);
  - a whole "Gone" card, from the Library (web: hover ×; TV: long-press).
    On TV a confirmation dialog guards the long-press. A hidden entry comes
    back on new activity (you re-watch the show, or the release is
    downloaded and reclaimed again), and never through search.
- **Remove a series from the Watchlist** (long-press + confirm on TV,
  hover × on web). Per-user, and reversible by nature: grabbing or
  playing an episode auto-recreates the follow.

### Fixed

- **The "N new" badge tells the truth.** It now counts episodes released
  since your last engagement with the series (page visit OR watch) —
  watching E04 consumes the already-out E05/E06, and a series you never
  opened no longer badges its entire back-catalogue ("48 new").
- **Collections no longer carry a leading "[GROUP]" in their title.** A
  release like `[H4KIG] Ted 2 (2015)` now titles its collection
  "Ted 2 (2015)" — the bracketed tag also broke the TMDB poster lookup.
  Existing collections self-heal at boot: title cleaned, poster
  re-resolved.
- **TV updater reliability.** The system installer's "Open" button is no
  longer greyed out, the app relaunches itself after an install when the
  device allows it, and when the installer launch gets swallowed the
  screen stays awake, the intent retries (or fires on return to
  foreground), and the focus lands on "Reopen installer" so the manual
  fallback is one press away.
- **TV Settings page scrolls** — the "More" card's buttons no longer get
  squashed into empty pills when the update card fills the screen.

### Changed

- **One "Discover" entry for the whole reco system** (web + TV): "For
  You" and "Tonight" are now tabs on a single page instead of two nav
  items; the legacy `/for-you` and `/moods` URLs redirect. The TV top
  bar slims down to Search · Discover · Library · Live TV · Settings —
  the Torrents and Watch history views moved into Settings (the update
  card keeps the top spot).
- **Home reordered for the "carry me" flow** (web + TV): Continue
  Watching → Watchlist → Library → recommendations, with the Watchlist
  sorted fresh-episodes-first.
- **Quieter home pages.** Shelf eyebrows that just restated the title
  ("For you", "On disk", "Following", "Recommended") are gone on web and
  TV; the useful counts stay. The web hero also drops the "Continue
  tonight · Resume" eyebrow, matching the TV.
- **Compact TV action chips.** Grab/Re-grab chips show just the language
  and the action at rest; quality, seeders and size appear on the focused
  chip (focus always precedes the click on a D-pad). Season-pack banners
  do the same with their metadata line.

## [1.3.0] - 2026-07-12

### Added

- **Ghost collections — nothing you watched ever vanishes.** When the
  disk-reclaim GC (or a cleanup) removes every torrent of a show or movie
  you've watched, its collection now stays visible **to you** (ghosts are
  scoped to each user's own watch history — nobody sees anyone else's):
  - **Library** keeps the card in place, greyed out with a "Gone" badge and
    a "No longer on disk" caption. Clicking it just opens the collection
    page — re-downloading stays a deliberate action there.
  - **Watch history** (web + Android TV) is now grouped by collection:
    poster + clean title as a header, one `S01E03 · 43%` line per episode,
    instead of a flat log of raw release names. Ghost collections stay
    listed and navigable.
  - **"Download again"** on a reclaimed history entry re-grabs the exact
    same release — same infohash, so playback resumes precisely where you
    left off, while the download streams in.
  - **The collection page lists "Previously on disk" releases** (web +
    Android TV) with the same "Download again" action — this is what makes
    a ghost **movie** recoverable (series could already re-grab from their
    episode offers). Nothing is automatic: opening a ghost never downloads
    anything by itself.
- **Android TV search: recent searches + TMDB suggestions.** The empty
  search screen offers your last 3 searches as one-click chips, and while
  you type, live TMDB suggestions (poster, title, year, TV/Movie) appear
  under the input — picking one searches the indexers with the canonical
  title and aligns the Type filter.
- **Theater mode on the web player.** The "Theater" button (or the `t`
  key) stretches the player across the full viewport width; the
  episodes/files panel restacks below it, like the responsive layout. The
  choice is remembered per device.

### Fixed

- **Wrong posters when a release hides its real title in the file names.**
  Collection identity now prefers the torrent's own name whenever it
  carries a structural marker (season or year) — a release like
  `Goblin.The.Lonely.and.Great.God.2016.…` whose files are just
  `Goblin S01E01.mkv` no longer resolves to the wrong TMDB entry.
  Existing mis-titled collections self-heal at boot (identity re-derived
  from the torrent names by consensus, poster re-resolved), and two
  collections that resolve to the same TMDB entity are automatically
  merged into one.

### Changed

- **Android TV home hero**: dropped the redundant "Continue tonight ·
  Resume" eyebrow (the Resume button says it all); the hero layout is
  unchanged.
- **Dependency refresh**: Compose BOM 2026.06.01 (picks up the upstream
  `ui-text` crash fix for long `maxLines` texts), `dompurify` 3.4.12,
  Rust lockfile patch bumps. `time` stays capped (`cookie` 0.18.1 still
  incompatible with 0.3.52+), api-gen stays on TypeScript 6 by design.

## [1.2.1] - 2026-07-06

### Added

- **Live TV channel search, across every country.** The channel grid (web +
  Android TV) gains a search box resolved server-side against the iptv-org
  channel database: type "bbc" from the France view and open BBC One directly.
  Matching is accent-insensitive and only playable channels are returned.
- **Android TV shows the escalation stage while a live channel connects**
  (hardware / software decoder / server transcode, with elapsed time), and
  Settings displays the exact build stamp of the installed APK.

### Fixed

- **Android TV kept streaming Live TV after leaving with the Home button** —
  the stream now stops immediately on Home and resumes at the live edge when
  you come back (the same lifecycle hole fixed for VOD in 1.1.0).
- **Stubborn interlaced/corrupt feeds (M6) now play on the TV** via a
  last-resort server-side deinterlace/transcode, engaged automatically only
  after both the hardware and software decoders failed; the working method is
  remembered per channel for a day, so later opens are instant.
- **Channel logos self-heal**: rate-limited logo hosts (imgur) no longer
  blank tiles for minutes; clients fall back to fetching the original logo
  directly, and failed lookups retry quickly instead of sticking.

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
  navigation is the native control-bar button (which _is_ reachable);
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
  size + seeders decided order — so a release merely _containing_ a query word
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
  two collections with the _same_ display title, and a new episode only ever
  attached to the half matching its release group — so it appeared "nowhere" on
  the half the user was watching. When an `anime:K` and a plain `K` collection
  resolve to the **same** TMDB id they now auto-merge into one (the legitimate
  same-title split — anime vs live-action _One Piece_, which carry _different_
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

[Unreleased]: https://github.com/uplg/iris/compare/1.4.0...HEAD
[1.4.0]: https://github.com/uplg/iris/compare/1.3.6...1.4.0
[1.3.6]: https://github.com/uplg/iris/compare/1.3.5...1.3.6
[1.3.5]: https://github.com/uplg/iris/compare/1.3.4...1.3.5
[1.3.4]: https://github.com/uplg/iris/compare/1.3.3...1.3.4
[1.3.3]: https://github.com/uplg/iris/compare/1.3.2...1.3.3
[1.3.2]: https://github.com/uplg/iris/compare/1.3.1...1.3.2
[1.3.1]: https://github.com/uplg/iris/compare/1.3.0...1.3.1
[1.3.0]: https://github.com/uplg/iris/compare/1.2.1...1.3.0
[1.2.1]: https://github.com/uplg/iris/compare/1.2.0...1.2.1
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
