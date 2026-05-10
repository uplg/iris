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
//! `collections.tmdb_id` (used by the UI to fetch poster / synopsis)
//! is written exclusively by [`enrich_after_verify`] — invoked from
//! `verify_tmdb_match` once we've matched the file's probed runtime
//! against TMDB's declared duration. That's the only signal strong
//! enough to justify pulling third-party metadata.
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

use crate::tmdb::TmdbClient;

/// Run after every successful ingest. Picks (or creates) the right
/// collection from SCENE-parsed identity, attaches the torrent, and
/// (for TV) populates `episode_files`. Best-effort: failures are
/// logged, not returned, since collection assignment is metadata
/// not playback.
///
/// Does NOT touch `collections.tmdb_id` — that field is reserved
/// for [`enrich_after_verify`] which writes it only after the
/// runtime-match probe has confirmed the indexer's tmdb hint.
///
/// `tmdb_id` is accepted for log/trace context but deliberately not
/// stored on the collection here. `_tmdb` is kept in the signature
/// for symmetry with future hooks; currently unused.
pub async fn assign_after_ingest(
    pool: &SqlitePool,
    _tmdb: Option<&TmdbClient>,
    infohash: &str,
    name: &str,
    tmdb_id: Option<i64>,
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

    let collection = match resolve_collection(pool, kind, name, identity).await {
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
        unverified_tmdb_hint = ?tmdb_id,
        "collection assigned",
    );

    // For TV: turn any SCENE-parseable filename into an episode_files
    // row so the Series page picks it up. Keyed on collection_id
    // (the SCENE identity), so a wrong tmdb_id can't poison some
    // unrelated Watchlist follow.
    if kind == Kind::Tv {
        for (file_idx, parsed) in &parsed_files {
            let Some(season) = parsed.season else { continue };
            let Some(episode) = parsed.episode else { continue };
            let _ = episode_files::upsert(
                pool,
                UpsertEpisodeFile {
                    collection_id: collection.id,
                    season: i64::from(season),
                    episode: i64::from(episode),
                    infohash: infohash.to_string(),
                    file_idx: *file_idx as i64,
                    derived_from: DerivedFrom::SceneParse,
                },
            )
            .await;
        }
    }
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
        if let Some((_, p)) = parsed_files.first() {
            if !p.title.is_empty() {
                return Some(p);
            }
        }
    }
    if let Some(p) = parsed_name {
        if !p.title.is_empty() {
            return Some(p);
        }
    }
    parsed_files.first().map(|(_, p)| p)
}

async fn resolve_collection(
    pool: &SqlitePool,
    kind: Kind,
    torrent_name: &str,
    identity: Option<&filename::Parsed>,
) -> Result<CollectionRow, sqlx::Error> {
    if let Some(p) = identity {
        let key = p.collection_key(kind == Kind::Tv);
        if !key.is_empty() {
            let display = p.display_with_year(kind == Kind::Tv);
            return collections::find_or_create(pool, &key, &display, kind).await;
        }
    }
    // Truly nothing parseable — standalone collection (one entry,
    // never merged) named after the raw torrent.
    collections::create_standalone(pool, torrent_name, kind).await
}

/// Stamp `collection.tmdb_id` from a torrent that has just been
/// runtime-verified. Called by `verify_tmdb_match` after flipping
/// `tmdb_verified=true`. First-writer-wins on the collection slot
/// — once a verified id is on file, a later torrent with a
/// different id can't overwrite it (would need explicit manual
/// re-tagging, future feature).
///
/// Also called from [`run_backfill`] for torrents that are already
/// verified at boot (the verification flag persists across
/// restarts, but the enrichment write didn't happen pre-rework).
pub async fn enrich_after_verify(pool: &SqlitePool, infohash: &str) {
    let row = match iris_db::torrents::find_by_infohash(pool, infohash).await {
        Ok(Some(r)) => r,
        Ok(None) => return,
        Err(e) => {
            tracing::warn!(error = %e, infohash, "enrich_after_verify: lookup failed");
            return;
        }
    };
    if !row.tmdb_verified {
        return;
    }
    let Some(tmdb_id) = row.tmdb_id else { return };
    let Some(collection_id) = row.collection_id else { return };
    if let Err(e) = collections::set_tmdb_id_if_missing(pool, collection_id, tmdb_id).await {
        tracing::warn!(error = %e, infohash, "enrich_after_verify: write failed");
        return;
    }
    tracing::info!(
        infohash,
        collection_id = %collection_id,
        tmdb_id,
        "collection enriched with verified tmdb_id",
    );
}

/// Walk every torrent currently in the library and assign a collection
/// to any that doesn't have one yet. Runs at boot to backfill the
/// existing library after the SCENE-first migration. Idempotent —
/// safe to call repeatedly.
pub async fn run_backfill(
    pool: &SqlitePool,
    tmdb: Option<&TmdbClient>,
    engine: &iris_torrent::Engine,
) {
    let rows = match iris_db::torrents::list_active(pool).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "collection backfill: list torrents failed");
            return;
        }
    };
    let mut done = 0;
    for row in rows {
        // Re-enrich verified torrents whose collection.tmdb_id is
        // still NULL (pre-rework databases, or torrents that got
        // verified after their initial assignment). No-op when the
        // slot is already filled.
        if row.collection_id.is_some() {
            if row.tmdb_verified {
                enrich_after_verify(pool, &row.infohash).await;
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
        let files: Vec<(usize, String)> = snap
            .files
            .into_iter()
            .map(|f| (f.index, f.path))
            .collect();
        assign_after_ingest(pool, tmdb, &row.infohash, &row.name, row.tmdb_id, &files).await;
        if row.tmdb_verified {
            enrich_after_verify(pool, &row.infohash).await;
        }
        done += 1;
    }
    if done > 0 {
        tracing::info!(count = done, "collection backfill complete");
    }
}
