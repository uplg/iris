// File-index and tmdb-id casts cross between i64 (DB) and usize (engine
// snapshot). All values bounded — see follows.rs for the same rationale.
#![allow(clippy::cast_sign_loss, clippy::cast_possible_wrap)]

//! Collection assignment — runs at ingest time AND as a one-shot
//! retroactive batch on existing torrents.
//!
//! Picks the right `collections` row (or creates one) for a torrent
//! using this priority:
//!   1. `tmdb_id` matches an existing collection → attach.
//!   2. SCENE-parsed series title matches an existing collection's
//!      `parsed_title_normalized` → attach.
//!   3. Otherwise create a new collection. With a `tmdb_id` we get the
//!      "TMDB-identity" path; without, the SCENE-title or standalone
//!      paths.
//!
//! For TV torrents we ALSO populate `episode_files` from any file whose
//! name parses to a `(season, episode)` — this is what lets the Series
//! page render an aggregated view without forcing the user to manually
//! tag each file.

use iris_db::SqlitePool;
use iris_db::collections::{self, Kind};
use iris_db::episode_files::{self, DerivedFrom, UpsertEpisodeFile};
use iris_media::filename;

use crate::tmdb::TmdbClient;

/// Run after every successful ingest. Assigns the torrent to a
/// collection and (for TV torrents) populates `episode_files` for any
/// SCENE-parseable file. Best-effort: failures are logged, not
/// returned, since collection assignment is metadata not playback.
pub async fn assign_after_ingest(
    pool: &SqlitePool,
    tmdb: Option<&TmdbClient>,
    infohash: &str,
    name: &str,
    tmdb_id: Option<i64>,
    files: &[(usize, String)],
) {
    let parsed_files: Vec<(usize, filename::Parsed)> = files
        .iter()
        .filter_map(|(idx, path)| {
            // The torrent file list ships full paths; the SCENE pattern
            // lives on the leaf name. `Show.Name.S01E02.../episode.mkv`
            // would mis-parse otherwise.
            let leaf = path.rsplit('/').next().unwrap_or(path);
            filename::parse(leaf).map(|p| (*idx, p))
        })
        .collect();

    let kind = guess_kind(&parsed_files, tmdb_id, name);
    let collection = match resolve_collection(pool, tmdb, tmdb_id, kind, name, &parsed_files).await {
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

    // For TV: turn any SCENE-parseable filename into an episode_files row
    // so the Series page picks it up. Movies skip this step (we don't
    // care about per-episode mapping for them).
    if kind == Kind::Tv {
        let Some(tid) = collection.tmdb_id.or(tmdb_id) else {
            // No TMDB id at the collection level — episode_files is
            // keyed on tmdb_id, so we can't store these mappings yet.
            // Re-runs of the assignment job after a manual TMDB tag
            // will pick them up.
            return;
        };
        for (file_idx, parsed) in &parsed_files {
            let Some(season) = parsed.season else { continue };
            let Some(episode) = parsed.episode else { continue };
            let _ = episode_files::upsert(
                pool,
                UpsertEpisodeFile {
                    tmdb_id: tid,
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

fn guess_kind(
    parsed: &[(usize, filename::Parsed)],
    tmdb_id: Option<i64>,
    _torrent_name: &str,
) -> Kind {
    // Any TV-shaped file inside the torrent → TV. Otherwise default to
    // Movie. We don't trust the torrent name alone since "Show Name S01"
    // packs and "Movie Name" releases share the same naming surface area;
    // the file-level SCENE marker is the high-signal cue.
    if parsed.iter().any(|(_, p)| p.is_tv()) {
        Kind::Tv
    } else {
        // tmdb_id alone doesn't tell us movie vs. tv (the search
        // payload's `kind` does, but we'd have to plumb it down here).
        // Default Movie is safe: a TMDB-tagged movie ends up in a
        // movie collection of size 1; a TMDB-tagged TV without scene-
        // parseable files still gets Movie, which is wrong but
        // recoverable manually.
        let _ = tmdb_id;
        Kind::Movie
    }
}

async fn resolve_collection(
    pool: &SqlitePool,
    tmdb: Option<&TmdbClient>,
    tmdb_id: Option<i64>,
    kind: Kind,
    torrent_name: &str,
    parsed_files: &[(usize, filename::Parsed)],
) -> Result<iris_db::collections::CollectionRow, sqlx::Error> {
    if let Some(tid) = tmdb_id {
        let display = if let Some(client) = tmdb {
            client
                .lookup(tid as u64)
                .await
                .map_or_else(|| torrent_name.to_string(), |m| m.title)
        } else {
            torrent_name.to_string()
        };
        return collections::find_or_create_by_tmdb(pool, tid, kind, &display).await;
    }
    // No TMDB id — fall back to SCENE title. Pick the title from the
    // first parseable file (they should all match within a torrent;
    // first-wins is a reasonable tiebreaker if not).
    if let Some((_, p)) = parsed_files.first() {
        let normalized = p.normalized_key();
        if !normalized.is_empty() {
            return collections::find_or_create_by_parsed_title(
                pool,
                &normalized,
                &p.title,
                kind,
            )
            .await;
        }
    }
    // Truly nothing to group on — standalone collection (one entry,
    // never merged).
    collections::create_standalone(pool, torrent_name, kind).await
}

/// Walk every torrent currently in the library and assign a collection
/// to any that doesn't have one yet. Run once at boot to backfill the
/// existing library when the Phase 4.5 migration lands. Idempotent —
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
        if row.collection_id.is_some() {
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
        done += 1;
    }
    if done > 0 {
        tracing::info!(count = done, "collection backfill complete");
    }
}
