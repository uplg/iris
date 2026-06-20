// File-index casts cross between i64 (DB) and usize (engine snapshot).
// All values bounded by the domain — see follows.rs for the same rationale.
#![allow(clippy::cast_sign_loss, clippy::cast_possible_wrap)]

//! Collection assignment — runs after every successful ingest AND as
//! a one-shot retroactive batch on existing torrents.
//!
//! Identity is SCENE-parsed: the torrent name (or, failing that, the
//! first parseable file leaf) yields a normalised key + display title
//! that anchors the collection. TMDB is **never** trusted at ingest
//! — neither for grouping, display, nor enrichment. The indexer-
//! attached `tmdb_id` is far too unreliable: it's wrong often enough
//! that propagating it to the collection produces cards with the
//! wrong show's poster or synopsis attached to a correctly-titled
//! folder.
//!
//! `collections.tmdb_id` (used by the UI to fetch poster / synopsis) is the
//! single source of truth, resolved from the collection's SCENE identity by
//! [`resolve_collection_tmdb`]. The runtime probe (`verify_tmdb_match`) only
//! flips the per-torrent `tmdb_verified` flag — it never writes the id.
//!
//! For TV torrents we also populate `episode_files` from any file
//! whose name parses to a `(season, episode)` — this is what lets
//! the Series page render an aggregated view without forcing the
//! user to manually tag each file. Keyed on `collection_id` (the
//! SCENE identity), not `tmdb_id`.

use iris_db::SqlitePool;
use iris_db::collections::{self, CollectionRow, Kind};
use iris_db::episode_files::{self, DerivedFrom, UpsertEpisodeFile};
use iris_media::filename;

use crate::anilist::AniListClient;
use crate::tmdb::TmdbClient;

/// Optional external-service handles threaded through the collection-
/// assignment + backfill paths. Bundled into one struct so the public
/// entry-points stay under clippy's argument-count bar and so adding a
/// future enrichment source doesn't churn every call site.
#[derive(Clone, Copy)]
pub struct EnrichDeps<'a> {
    pub tmdb: Option<&'a TmdbClient>,
    pub anilist: Option<&'a AniListClient>,
    pub providers: Option<&'a iris_providers::ProviderRegistry>,
}

/// Run after every successful ingest. Picks (or creates) the right
/// collection from SCENE-parsed identity, attaches the torrent, and
/// (for TV) populates `episode_files`. Best-effort: failures are
/// logged, not returned, since collection assignment is metadata
/// not playback.
///
/// Resolves the collection's `tmdb_id` from its SCENE identity via
/// [`resolve_collection_tmdb`] — THE single resolution path (movies + TV).
/// The runtime probe (`verify_tmdb_match`) later only confirms it; the
/// torrent's own id is never consulted.
pub async fn assign_after_ingest(
    pool: &SqlitePool,
    deps: EnrichDeps<'_>,
    infohash: &str,
    name: &str,
    files: &[(usize, String)],
) {
    // Parse the torrent name first — it's the highest-signal SCENE
    // string we have (indexers consistently SCENE-name top-level
    // releases). Then parse each file leaf in case the torrent name
    // didn't carry season/episode info but the files do.
    //
    // Filter the file list to playable video files in non-sample
    // paths: an `Show.S01E02.nfo` parses to the same (S, E) as the
    // real episode and would steal its `episode_files` slot via the
    // UNIQUE(infohash, file_idx) write order; samples (`/sample/…`,
    // `*.sample.mkv`) are also tagged S01E02 by SCENE convention and
    // would land users on a 50 MB clip if picked first.
    let parsed_name = filename::parse(name);
    let parsed_files: Vec<(usize, filename::Parsed)> = files
        .iter()
        .filter(|(_, path)| is_main_video_file(path))
        .filter_map(|(idx, path)| {
            let leaf = path.rsplit('/').next().unwrap_or(path);
            filename::parse(leaf).map(|p| (*idx, p))
        })
        .collect();

    let kind = guess_kind(parsed_name.as_ref(), &parsed_files);
    let identity = pick_identity(kind, parsed_name.as_ref(), &parsed_files);

    // Anime classification (offline, naming-gated). Decided here because
    // it's baked into the collection identity key at create-time — an
    // anime and a live-action show sharing a title must not merge. The
    // async AniList/TMDB confirm step only ever strengthens this later.
    // Check the torrent name plus every video leaf so a fansub group
    // token on either surface counts.
    let id_season = identity.and_then(|p| p.season);
    let id_episode = identity.and_then(|p| p.episode);
    let is_anime = kind == Kind::Tv
        && (filename::looks_like_anime_release(name, id_season, id_episode)
            || files.iter().any(|(_, path)| {
                let leaf = path.rsplit('/').next().unwrap_or(path);
                filename::looks_like_anime_release(leaf, id_season, id_episode)
            }));

    let collection = match resolve_collection(pool, kind, name, identity, is_anime).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, infohash, "collection assign: resolve failed");
            return;
        }
    };

    if let Err(e) = iris_db::torrents::set_collection(pool, infohash, Some(collection.id)).await {
        tracing::warn!(error = %e, infohash, "collection assign: set_collection failed");
        return;
    }
    tracing::info!(
        infohash,
        collection_id = %collection.id,
        kind = collection.kind,
        title = %collection.display_title,
        "collection assigned",
    );

    // THE single tmdb resolution path: from the collection's SCENE identity
    // (`display_title`), for movies AND TV alike. No torrent-level resolution
    // duplicates this. (TV additionally pre-warms episodes/anime below.)
    resolve_collection_tmdb(pool, deps, &collection, kind).await;

    // For TV: turn any SCENE-parseable filename into an episode_files
    // row so the Series page picks it up. Keyed on collection_id
    // (the SCENE identity), so a wrong tmdb_id can't poison some
    // unrelated Watchlist follow.
    if kind == Kind::Tv {
        for (file_idx, parsed) in &parsed_files {
            let Some(season) = parsed.season else {
                continue;
            };
            let Some(episode) = parsed.episode else {
                continue;
            };
            let _ = episode_files::upsert(
                pool,
                UpsertEpisodeFile {
                    collection_id: collection.id,
                    season: i64::from(season),
                    episode: i64::from(episode),
                    infohash: infohash.to_string(),
                    file_idx: *file_idx as i64,
                    derived_from: DerivedFrom::SceneParse,
                    // Absolute number only for genuinely-fleuve releases
                    // (threshold-gated inside the helper); seasonal anime
                    // and ordinary TV stay `None`.
                    absolute_episode: filename::absolute_from_parsed(parsed).map(i64::from),
                },
            )
            .await;
        }
        // Make the first visit useful: the user clicked "Add to
        // library", they're about to land on a freshly-created
        // collection page that would otherwise show "No poster" +
        // empty Watchlist until the runtime probe + 4 h scheduler
        // tick caught up. Both signals are cheap to pre-warm:
        //   * collection identity → TMDB resolve gives us a `tmdb_id`
        //     good enough for the poster lookup (NOT `tmdb_verified` —
        //     that still requires the runtime probe).
        //   * A one-shot scan against the indexers populates
        //     `available_episodes` so the "next episodes" picker
        //     has data on first render.
        prewarm_tv_collection(pool, deps, &collection).await;
    }
}

/// THE single TMDB-id resolution path for a collection: resolve from its SCENE
/// identity (`display_title`) and stamp it (first-writer-wins, so `tmdb_backfill`
/// or a prior grab is never clobbered). Runs for movies AND TV. The result is
/// unverified (poster-grade) until the runtime probe confirms it in
/// `verify_tmdb_match`. The torrent's own name is deliberately never used —
/// c411 season packs are named "Saison N" (no title) and resolve to garbage.
async fn resolve_collection_tmdb(
    pool: &SqlitePool,
    deps: EnrichDeps<'_>,
    collection: &iris_db::collections::CollectionRow,
    kind: Kind,
) {
    if collection.tmdb_id.is_some() {
        return;
    }
    let Some(client) = deps.tmdb else { return };
    let hint = if kind == Kind::Tv {
        crate::tmdb::TmdbKind::Tv
    } else {
        crate::tmdb::TmdbKind::Movie
    };
    let Some(resolved) =
        crate::tmdb_resolve::resolve_release_name(pool, client, &collection.display_title, Some(hint))
            .await
    else {
        return;
    };
    let Ok(id) = i64::try_from(resolved.tmdb_id) else {
        tracing::warn!(
            tmdb_id = resolved.tmdb_id,
            collection_id = %collection.id,
            "resolve_collection_tmdb: id overflowed i64 (shouldn't happen)",
        );
        return;
    };
    if let Err(e) = iris_db::collections::set_tmdb_id_if_missing(pool, collection.id, id).await {
        tracing::warn!(error = %e, collection_id = %collection.id, "resolve_collection_tmdb: write failed");
    } else {
        tracing::info!(
            collection_id = %collection.id,
            tmdb_id = id,
            "resolved collection TMDB id from identity (unverified — poster only)",
        );
    }
}

/// Best-effort: enrich a TV collection's anime id and kick the collections
/// scheduler against the brand-new collection so the user's first visit sees a
/// populated "available episodes" panel. Tolerant of failure — the periodic
/// scheduler tick still runs independently and fills any gap; the point is just
/// to shorten the visible "empty" window from minutes to seconds. (The TMDB id
/// itself is resolved up in `assign_after_ingest` via `resolve_collection_tmdb`.)
async fn prewarm_tv_collection(
    pool: &SqlitePool,
    deps: EnrichDeps<'_>,
    collection: &iris_db::collections::CollectionRow,
) {
    // Anime enrichment: when the offline classifier already flagged
    // this collection anime, attach an AniList id (poster /
    // recommendations metadata). AniList matches a *title*, not a
    // specific release, so it can only enrich here — the per-release
    // anime/live-action split is decided by the offline signal at
    // ingest. Best-effort; missing AniList just leaves `anilist_id` null.
    if collection.is_anime
        && collection.anilist_id.is_none()
        && let Some(id) = anilist_id_for(deps.anilist, &collection.display_title).await
    {
        if let Err(e) =
            iris_db::collections::set_is_anime(pool, collection.id, true, Some(id)).await
        {
            tracing::warn!(error = %e, collection_id = %collection.id, "prewarm: set anilist_id failed");
        } else {
            tracing::info!(
                collection_id = %collection.id,
                anilist_id = id,
                "prewarm: enriched anime collection with AniList id",
            );
        }
    }
    // The collection's TMDB id is resolved once, up in `assign_after_ingest`
    // via `resolve_collection_tmdb` (movies + TV) — not here, to keep a single
    // resolution path. This prewarm only kicks the episode scheduler below.
    if let Some(reg) = deps.providers
        && let Err(e) =
            crate::collections_scheduler::scan_collection(pool, reg, collection.id).await
    {
        tracing::warn!(
            error = %e,
            collection_id = %collection.id,
            "prewarm: initial scheduler scan failed",
        );
    }
}

/// Best AniList media id for a series title, or `None`. AniList's
/// `SEARCH_MATCH` sort already ranks the closest title first; we only
/// accept a hit whose normalised title actually matches the query so a
/// fuzzy near-miss doesn't attach the wrong show's id. TV shows only
/// (`!is_movie`).
async fn anilist_id_for(anilist: Option<&AniListClient>, title: &str) -> Option<i64> {
    let client = anilist?;
    let want = filename::series_key(title);
    client
        .search(title)
        .await
        .into_iter()
        .find(|m| !m.is_movie && filename::series_key(&m.title) == want)
        .map(|m| m.anilist_id)
}

/// True when `path` is a real video file we'd want to play —
/// excludes NFO / SRT / sample subdirectories. Matches the playable
/// extensions used elsewhere (largest-video picker in
/// `routes/follows.rs`); kept inline here to avoid pulling that
/// module into our dependency graph.
fn is_main_video_file(path: &str) -> bool {
    const VIDEO_EXTS: [&str; 10] = [
        "mkv", "mp4", "webm", "m4v", "avi", "mov", "ts", "mts", "m2ts", "wmv",
    ];
    let lower = path.to_ascii_lowercase();
    if lower.contains("/sample/") || lower.contains(".sample.") {
        return false;
    }
    let ext = std::path::Path::new(&lower)
        .extension()
        .and_then(|e| e.to_str());
    ext.is_some_and(|e| VIDEO_EXTS.contains(&e))
}

fn guess_kind(
    parsed_name: Option<&filename::Parsed>,
    parsed_files: &[(usize, filename::Parsed)],
) -> Kind {
    // Any TV-shaped file inside the torrent → TV. Falls back to the
    // torrent name when files aren't parseable (rare; fan encodes
    // with custom names sometimes lose SCENE structure). Default
    // Movie when nothing tells us otherwise.
    if parsed_files.iter().any(|(_, p)| p.is_tv()) {
        return Kind::Tv;
    }
    if parsed_name.is_some_and(filename::Parsed::is_tv) {
        return Kind::Tv;
    }
    Kind::Movie
}

/// Pick the canonical Parsed used to derive collection identity.
///
/// For TV: prefer the first SCENE-parseable file leaf. Season packs
/// name themselves `Show.S02.COMPLETE.1080p…` — `S02` alone (no E)
/// doesn't satisfy `find_se_marker`, so the parser tail-trims to
/// "1080p" and the title becomes `Show S02 COMPLETE` (cruft). The
/// individual files inside the pack each have proper `S02EXX`
/// markers and parse to a clean title — using one of them keeps
/// season packs in the same collection as standalone-episode
/// torrents.
///
/// For movies: the torrent name wins when usable (it's the only
/// thing carrying the year — files are sometimes renamed to
/// `Movie.mkv` with no year, which would lose Dune-1984 vs Dune-2021
/// disambiguation). Falls back to the first file leaf if torrent
/// name didn't parse.
fn pick_identity<'a>(
    kind: Kind,
    parsed_name: Option<&'a filename::Parsed>,
    parsed_files: &'a [(usize, filename::Parsed)],
) -> Option<&'a filename::Parsed> {
    if kind == Kind::Tv {
        // File names usually carry the most canonical SCENE form for
        // TV releases, BUT only when our parser actually recognised
        // a season marker. A file like
        // `Silicon Valley - 1x01 - Minimum Viable Product.mkv`
        // uses the Plex-style `NxNN` convention the SCENE parser
        // doesn't recognise — the parse falls through and the
        // whole filename ends up in `title`. Letting that drive
        // the collection's display_title leaks junk like
        // "Silicon Valley - 1x01 - Minimum Viable Product Multi Papaya"
        // into the UI. Only trust the file parse when it produced
        // a real season; otherwise fall back to the torrent name
        // (which for season packs is canonical: `Silicon.Valley.S01.…`).
        if let Some((_, p)) = parsed_files.first()
            && !p.title.is_empty()
            && p.season.is_some()
        {
            return Some(p);
        }
    }
    if let Some(p) = parsed_name
        && !p.title.is_empty()
    {
        return Some(p);
    }
    parsed_files.first().map(|(_, p)| p)
}

async fn resolve_collection(
    pool: &SqlitePool,
    kind: Kind,
    torrent_name: &str,
    identity: Option<&filename::Parsed>,
    is_anime: bool,
) -> Result<CollectionRow, sqlx::Error> {
    if let Some(p) = identity {
        let key = p.collection_key_kind(kind == Kind::Tv, is_anime);
        if !key.is_empty() {
            let display = p.display_with_year(kind == Kind::Tv);
            return collections::find_or_create(pool, &key, &display, kind, is_anime).await;
        }
    }
    // Truly nothing parseable — standalone collection (one entry,
    // never merged) named after the raw torrent.
    collections::create_standalone(pool, torrent_name, kind).await
}

/// Re-derive `scene_parse` episode rows for a torrent that already has
/// a collection, correcting any `(season, episode)` that drifted
/// because the filename parser improved since the row was first
/// written. The motivating case: season packs whose leaves space the
/// markers (`Show - S02 E02.mkv`) used to parse as a season pack
/// (episode 0) and every leaf rendered `S02E00`; the parser now reads
/// the spaced form, and this pass retro-corrects already-ingested
/// packs the insert-only `upsert` can't touch.
///
/// Only `scene_parse` rows are rewritten (see
/// [`episode_files::correct_scene_parsed`]); user-/tmdb-derived rows
/// are left alone. Idempotent and effectively free once converged.
async fn reconcile_scene_episodes(pool: &SqlitePool, infohash: &str, files: &[(usize, String)]) {
    let mut fixed = 0u32;
    for (idx, path) in files {
        if !is_main_video_file(path) {
            continue;
        }
        let leaf = path.rsplit('/').next().unwrap_or(path);
        let Some(parsed) = filename::parse(leaf) else {
            continue;
        };
        let (Some(season), Some(episode)) = (parsed.season, parsed.episode) else {
            continue;
        };
        match episode_files::correct_scene_parsed(
            pool,
            infohash,
            *idx as i64,
            i64::from(season),
            i64::from(episode),
        )
        .await
        {
            Ok(true) => fixed += 1,
            Ok(false) => {}
            Err(e) => tracing::warn!(error = %e, infohash, "episode reconcile: update failed"),
        }
    }
    if fixed > 0 {
        tracing::info!(
            infohash,
            fixed,
            "scene episode rows re-derived after parser change",
        );
    }
}

/// Boot-time repair for TV collections whose `parsed_title_normalized`
/// and `display_title` were set from a junk file-leaf parse by an
/// older [`pick_identity`] (which trusted any non-empty file title,
/// even when no season was found). Today's `pick_identity` requires
/// a season-marked file parse and falls back to the torrent name —
/// this self-heal back-applies the same rule to rows already on disk.
///
/// Conservative on purpose:
/// - Movies are left alone (their identity isn't affected by the
///   `pick_identity` bug fix).
/// - Skipped when the canonical key would collide with another
///   existing TV collection (that's a merge we don't auto-resolve).
/// - Only repairs rows whose torrent name parses with a season —
///   without that we have no canonical key to write.
async fn heal_tv_collection_identity(pool: &SqlitePool, infohash: &str) {
    let Ok(Some(torrent)) = iris_db::torrents::find_by_infohash(pool, infohash).await else {
        return;
    };
    let Some(collection_id) = torrent.collection_id else {
        return;
    };
    let Ok(Some(collection)) = iris_db::collections::get(pool, collection_id).await else {
        return;
    };
    if collection.kind != "tv" {
        return;
    }
    let Some(parsed) = filename::parse(&torrent.name) else {
        return;
    };
    if parsed.season.is_none() {
        return;
    }
    let new_key = parsed.collection_key(true);
    if new_key.is_empty() {
        return;
    }
    let current_key = collection.parsed_title_normalized.as_deref().unwrap_or("");
    if current_key == new_key {
        return; // already canonical
    }
    // A different collection already owns the canonical key — would
    // need a torrent-migration to merge; defer instead of corrupting
    // the existing row. debug, not warn: this is a benign, permanent
    // state (e.g. a legacy "silicon valley s01 multi" row next to the
    // canonical "silicon valley") re-evaluated on every backfill tick,
    // so at warn it reprints every 5 min forever with nothing to act on.
    if let Ok(Some(other)) =
        iris_db::collections::find_by_parsed_title(pool, &new_key, Kind::Tv).await
        && other.id != collection_id
    {
        tracing::debug!(
            collection_id = %collection_id,
            current = %current_key,
            target = %new_key,
            other_id = %other.id,
            "heal_tv_collection_identity: target key already owned by another collection — skipping",
        );
        return;
    }
    let new_display = parsed.display_with_year(true);
    if let Err(e) =
        iris_db::collections::set_parsed_title_normalized(pool, collection_id, &new_key).await
    {
        tracing::warn!(error = %e, collection_id = %collection_id, "heal: set_parsed_title_normalized failed");
        return;
    }
    if let Err(e) = iris_db::collections::set_display_title(pool, collection_id, &new_display).await
    {
        tracing::warn!(error = %e, collection_id = %collection_id, "heal: set_display_title failed");
        return;
    }
    tracing::info!(
        collection_id = %collection_id,
        old_key = %current_key,
        new_key = %new_key,
        new_display = %new_display,
        "TV collection identity self-healed from torrent name",
    );
}

/// Boot/periodic self-heal for the anime/live-action amalgam. Before
/// the anime-aware identity work an anime fansub release and a
/// same-titled live-action show collapsed into one `collections` row
/// (the reported *One Piece* bug). This re-classifies a torrent and,
/// when its anime-aware key disagrees with the collection it currently
/// sits in, either renames the row in place (a pure, mis-keyed anime
/// collection) or moves just this torrent out of a mixed row into the
/// correct `anime:`-prefixed collection — then re-homes its episode
/// files (with absolute numbers) and rebuilds the stale offer cache.
///
/// Only ever invoked for torrents the offline classifier flags anime,
/// so it never touches non-anime identities (those keep going through
/// [`heal_tv_collection_identity`]). Conservative + idempotent: once a
/// torrent is in its correct collection the early return fires and the
/// expensive availability rebuild never runs again.
async fn heal_anime_collection_identity(
    pool: &SqlitePool,
    providers: Option<&iris_providers::ProviderRegistry>,
    infohash: &str,
    files: &[(usize, String)],
) {
    let Ok(Some(torrent)) = iris_db::torrents::find_by_infohash(pool, infohash).await else {
        return;
    };
    let Some(collection_id) = torrent.collection_id else {
        return;
    };
    let Ok(Some(collection)) = collections::get(pool, collection_id).await else {
        return;
    };
    if collection.kind != "tv" {
        return;
    }
    let Some(parsed) = filename::parse(&torrent.name) else {
        return;
    };
    if parsed.season.is_none() {
        return; // no season marker → no canonical TV key to write
    }
    let is_anime = filename::looks_like_anime_release(&torrent.name, parsed.season, parsed.episode);
    let new_key = parsed.collection_key_kind(true, is_anime);
    if new_key.is_empty() {
        return;
    }
    let current_key = collection
        .parsed_title_normalized
        .clone()
        .unwrap_or_default();

    // Already in the correct collection — keep the denormalised flag and
    // absolute numbers current, nothing else to do (steady state).
    if current_key == new_key {
        if collection.is_anime != is_anime {
            let _ = collections::set_is_anime(pool, collection_id, is_anime, None).await;
        }
        backfill_episode_absolutes(pool, infohash, files).await;
        return;
    }

    AnimeHeal {
        pool,
        providers,
        infohash,
        files,
        parsed,
        is_anime,
        new_key,
        source_id: collection_id,
        current_key,
    }
    .relocate()
    .await;
}

/// Bundled context for the anime/live-action heal so the rename / move
/// branch helpers stay under clippy's argument-count bar.
struct AnimeHeal<'a> {
    pool: &'a SqlitePool,
    providers: Option<&'a iris_providers::ProviderRegistry>,
    infohash: &'a str,
    files: &'a [(usize, String)],
    parsed: filename::Parsed,
    is_anime: bool,
    new_key: String,
    source_id: uuid::Uuid,
    current_key: String,
}

impl AnimeHeal<'_> {
    /// Decide rename-in-place vs move-out and run it. Rename only when
    /// the WHOLE source collection resolves to the new key (every
    /// sibling) and nothing else already owns it — otherwise a mixed row
    /// (anime + live-action) would drag the live-action along.
    async fn relocate(self) {
        let siblings = iris_db::torrents::list_in_collection(self.pool, self.source_id)
            .await
            .unwrap_or_default();
        let all_match_new = siblings.iter().all(|t| {
            filename::parse(&t.name).is_some_and(|p| {
                p.season.is_some()
                    && p.collection_key_kind(
                        true,
                        filename::looks_like_anime_release(&t.name, p.season, p.episode),
                    ) == self.new_key
            })
        });
        let existing_target = collections::find_by_parsed_title(self.pool, &self.new_key, Kind::Tv)
            .await
            .ok()
            .flatten();
        if all_match_new && existing_target.is_none() {
            self.rename_in_place().await;
        } else {
            self.move_out(existing_target).await;
        }
    }

    /// Pure, uncollided source row → rewrite its key / title / flag.
    async fn rename_in_place(&self) {
        let display = self.parsed.display_with_year(true);
        if collections::set_parsed_title_normalized(self.pool, self.source_id, &self.new_key)
            .await
            .is_err()
        {
            return;
        }
        let _ = collections::set_display_title(self.pool, self.source_id, &display).await;
        let _ = collections::set_is_anime(self.pool, self.source_id, self.is_anime, None).await;
        backfill_episode_absolutes(self.pool, self.infohash, self.files).await;
        tracing::info!(
            collection_id = %self.source_id,
            old_key = %self.current_key,
            new_key = %self.new_key,
            is_anime = self.is_anime,
            "anime identity self-healed (renamed in place)",
        );
        rebuild_availability(self.pool, self.providers, &self.current_key, self.source_id).await;
    }

    /// Mixed row / target exists → move just this torrent + its files
    /// into the correct collection.
    async fn move_out(&self, existing_target: Option<CollectionRow>) {
        let target = match existing_target {
            Some(c) => c,
            None => match collections::find_or_create(
                self.pool,
                &self.new_key,
                &self.parsed.display_with_year(true),
                Kind::Tv,
                self.is_anime,
            )
            .await
            {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(error = %e, "anime heal: create target collection failed");
                    return;
                }
            },
        };
        if target.id == self.source_id {
            return;
        }
        if iris_db::torrents::set_collection(self.pool, self.infohash, Some(target.id))
            .await
            .is_err()
        {
            return;
        }
        // Re-home episode files: drop the rows under the old collection
        // and re-derive them (with absolutes) under the new one.
        let _ = episode_files::delete_for_infohash(self.pool, self.infohash).await;
        for (file_idx, path) in self.files {
            if !is_main_video_file(path) {
                continue;
            }
            let leaf = path.rsplit('/').next().unwrap_or(path);
            let Some(p) = filename::parse(leaf) else {
                continue;
            };
            let (Some(season), Some(episode)) = (p.season, p.episode) else {
                continue;
            };
            let _ = episode_files::upsert(
                self.pool,
                UpsertEpisodeFile {
                    collection_id: target.id,
                    season: i64::from(season),
                    episode: i64::from(episode),
                    infohash: self.infohash.to_string(),
                    file_idx: *file_idx as i64,
                    derived_from: DerivedFrom::SceneParse,
                    absolute_episode: filename::absolute_from_parsed(&p).map(i64::from),
                },
            )
            .await;
        }
        tracing::info!(
            infohash = self.infohash,
            from = %self.source_id,
            to = %target.id,
            new_key = %self.new_key,
            is_anime = self.is_anime,
            "anime identity self-healed (torrent moved out of mixed collection)",
        );
        // The old shared name carried cross-entity offers — wipe both
        // sides and rescan under their now-correct identities.
        rebuild_availability(self.pool, self.providers, &self.current_key, self.source_id).await;
        rebuild_availability(self.pool, self.providers, &self.new_key, target.id).await;
    }
}

/// Back-fill `absolute_episode` on a torrent's already-stored
/// `scene_parse` episode rows. Idempotent — a no-op once the absolute
/// numbers have converged (the SQL guard returns 0 rows affected).
async fn backfill_episode_absolutes(pool: &SqlitePool, infohash: &str, files: &[(usize, String)]) {
    for (idx, path) in files {
        if !is_main_video_file(path) {
            continue;
        }
        let leaf = path.rsplit('/').next().unwrap_or(path);
        let Some(p) = filename::parse(leaf) else {
            continue;
        };
        let (Some(season), Some(episode)) = (p.season, p.episode) else {
            continue;
        };
        let _ = episode_files::correct_scene_parsed_with_absolute(
            pool,
            infohash,
            *idx as i64,
            i64::from(season),
            i64::from(episode),
            filename::absolute_from_parsed(&p).map(i64::from),
        )
        .await;
    }
}

/// Drop the stale cached offers stored under `normalized` and trigger an
/// immediate re-scan of `collection_id` so availability repopulates
/// under the correct (possibly newly-split) identity. Only called from
/// the heal transition, never in steady state.
async fn rebuild_availability(
    pool: &SqlitePool,
    providers: Option<&iris_providers::ProviderRegistry>,
    normalized: &str,
    collection_id: uuid::Uuid,
) {
    let _ = iris_db::available_episodes::delete_for_series(pool, normalized).await;
    if let Some(reg) = providers
        && let Err(e) =
            crate::collections_scheduler::scan_collection(pool, reg, collection_id).await
    {
        tracing::warn!(error = %e, collection_id = %collection_id, "anime heal: rescan failed");
    }
}

/// Walk every torrent currently in the library and assign a collection
/// to any that doesn't have one yet. Runs at boot to backfill the
/// existing library after the SCENE-first migration. Idempotent —
/// safe to call repeatedly.
pub async fn run_backfill(pool: &SqlitePool, deps: EnrichDeps<'_>, engine: &iris_torrent::Engine) {
    let rows = match iris_db::torrents::list_active(pool).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "collection backfill: list torrents failed");
            return;
        }
    };
    let mut done = 0;
    for row in rows {
        if row.collection_id.is_some() {
            // (collection.tmdb_id is filled by `tmdb_backfill` from the
            // collection's identity — the single resolution path.)
            // Self-heal stale episode numbers from a since-improved
            // parser. Needs the engine file list; torrents not yet
            // loaded are retried on a later tick (same as the
            // assignment path below).
            let files: Option<Vec<(usize, String)>> = engine
                .get_by_infohash(&row.infohash)
                .map(|snap| snap.files.into_iter().map(|f| (f.index, f.path)).collect());
            if let Some(files) = &files {
                reconcile_scene_episodes(pool, &row.infohash, files).await;
            }
            // Route by anime classification so the two heals never fight
            // over the same key: an anime release goes through the
            // anime-aware split/rename + absolute backfill; everything
            // else keeps the original junk-title repair.
            let is_anime_torrent = filename::parse(&row.name).is_some_and(|p| {
                filename::looks_like_anime_release(&row.name, p.season, p.episode)
            });
            if is_anime_torrent {
                // Needs the file list; torrents whose engine state isn't
                // loaded yet are retried on a later tick.
                if let Some(files) = &files {
                    heal_anime_collection_identity(pool, deps.providers, &row.infohash, files)
                        .await;
                }
            } else {
                // Self-heal TV collection identity. Earlier builds picked
                // the first file's parse without checking for a season
                // marker, so Plex-style `NxNN` filenames produced junk
                // `display_title` like "Silicon Valley - 1x01 - Minimum
                // Viable Product Multi Papaya" when the torrent name
                // (`Silicon.Valley.S01.…`) was the right answer.
                heal_tv_collection_identity(pool, &row.infohash).await;
            }
            continue;
        }
        // Need the file list to detect TV-vs-movie via SCENE parsing.
        // Pull from the live engine snapshot (same place the API serves
        // it from); torrents whose engine state isn't loaded yet get
        // skipped this round and re-tried on the next boot.
        let Some(snap) = engine.get_by_infohash(&row.infohash) else {
            continue;
        };
        let files: Vec<(usize, String)> =
            snap.files.into_iter().map(|f| (f.index, f.path)).collect();
        assign_after_ingest(pool, deps, &row.infohash, &row.name, &files).await;
        done += 1;
    }
    if done > 0 {
        tracing::info!(count = done, "collection backfill complete");
    }
}
