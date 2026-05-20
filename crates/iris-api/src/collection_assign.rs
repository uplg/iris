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
    tmdb: Option<&TmdbClient>,
    providers: Option<&iris_providers::ProviderRegistry>,
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
        // Make the first visit useful: the user clicked "Add to
        // library", they're about to land on a freshly-created
        // collection page that would otherwise show "No poster" +
        // empty Watchlist until the runtime probe + 4 h scheduler
        // tick caught up. Both signals are cheap to pre-warm:
        //   * SCENE-name → TMDB resolve gives us a `tmdb_id` good
        //     enough for the poster lookup (NOT `tmdb_verified` —
        //     that still requires the runtime probe).
        //   * A one-shot scan against the indexers populates
        //     `available_episodes` so the "next episodes" picker
        //     has data on first render.
        prewarm_tv_collection(pool, tmdb, providers, &collection, name).await;
    }
}

/// Best-effort: resolve a TMDB id from the SCENE name and kick the
/// collections scheduler against the brand-new collection so the
/// user's first visit to the collection page sees a poster + a
/// populated "available episodes" panel.
///
/// Both operations are tolerant of failure — the runtime probe
/// (`enrich_after_verify`) and the periodic scheduler tick still run
/// independently and will eventually fill the gaps. The point of
/// this pre-warm is just to shorten the visible "empty" window from
/// minutes to seconds.
async fn prewarm_tv_collection(
    pool: &SqlitePool,
    tmdb: Option<&TmdbClient>,
    providers: Option<&iris_providers::ProviderRegistry>,
    collection: &iris_db::collections::CollectionRow,
    release_name: &str,
) {
    if collection.tmdb_id.is_none() {
        if let Some(client) = tmdb {
            if let Some(resolved) = crate::tmdb_resolve::resolve_release_name(
                pool,
                client,
                release_name,
                Some(crate::tmdb::TmdbKind::Tv),
            )
            .await
            {
                // TMDB ids never exceed ~i32::MAX in practice; reject
                // anything that doesn't fit i64 cleanly rather than
                // forcing an `as` cast — keeps `cargo clippy` happy
                // without an `allow` blanket.
                let Ok(id) = i64::try_from(resolved.tmdb_id) else {
                    tracing::warn!(
                        tmdb_id = resolved.tmdb_id,
                        collection_id = %collection.id,
                        "prewarm: TMDB id overflowed i64 — dropping (shouldn't happen)",
                    );
                    return;
                };
                if let Err(e) =
                    iris_db::collections::set_tmdb_id_if_missing(pool, collection.id, id).await
                {
                    tracing::warn!(
                        error = %e,
                        collection_id = %collection.id,
                        "prewarm: set_tmdb_id_if_missing failed",
                    );
                } else {
                    tracing::info!(
                        collection_id = %collection.id,
                        tmdb_id = id,
                        "prewarm: resolved TMDB id via SCENE name (unverified — poster only)",
                    );
                }
            }
        }
    }
    if let Some(reg) = providers {
        if let Err(e) =
            crate::collections_scheduler::scan_collection(pool, reg, collection.id).await
        {
            tracing::warn!(
                error = %e,
                collection_id = %collection.id,
                "prewarm: initial scheduler scan failed",
            );
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
        if let Some((_, p)) = parsed_files.first() {
            if !p.title.is_empty() && p.season.is_some() {
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
    tracing::debug!(
        infohash,
        collection_id = %collection_id,
        tmdb_id,
        "collection enriched with verified tmdb_id",
    );
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
    let Some(collection_id) = torrent.collection_id else { return };
    let Ok(Some(collection)) = iris_db::collections::get(pool, collection_id).await else {
        return;
    };
    if collection.kind != "tv" {
        return;
    }
    let Some(parsed) = filename::parse(&torrent.name) else { return };
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
    // the existing row.
    if let Ok(Some(other)) =
        iris_db::collections::find_by_parsed_title(pool, &new_key, Kind::Tv).await
    {
        if other.id != collection_id {
            tracing::warn!(
                collection_id = %collection_id,
                current = %current_key,
                target = %new_key,
                other_id = %other.id,
                "heal_tv_collection_identity: target key already owned by another collection — skipping",
            );
            return;
        }
    }
    let new_display = parsed.display_with_year(true);
    if let Err(e) =
        iris_db::collections::set_parsed_title_normalized(pool, collection_id, &new_key).await
    {
        tracing::warn!(error = %e, collection_id = %collection_id, "heal: set_parsed_title_normalized failed");
        return;
    }
    if let Err(e) =
        iris_db::collections::set_display_title(pool, collection_id, &new_display).await
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

/// Walk every torrent currently in the library and assign a collection
/// to any that doesn't have one yet. Runs at boot to backfill the
/// existing library after the SCENE-first migration. Idempotent —
/// safe to call repeatedly.
pub async fn run_backfill(
    pool: &SqlitePool,
    tmdb: Option<&TmdbClient>,
    providers: Option<&iris_providers::ProviderRegistry>,
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
            // Self-heal stale episode numbers from a since-improved
            // parser. Needs the engine file list; torrents not yet
            // loaded are retried on a later tick (same as the
            // assignment path below).
            if let Some(snap) = engine.get_by_infohash(&row.infohash) {
                let files: Vec<(usize, String)> =
                    snap.files.into_iter().map(|f| (f.index, f.path)).collect();
                reconcile_scene_episodes(pool, &row.infohash, &files).await;
            }
            // Self-heal TV collection identity. Earlier builds picked
            // the first file's parse without checking for a season
            // marker, so Plex-style `NxNN` filenames produced junk
            // `display_title` like "Silicon Valley - 1x01 - Minimum
            // Viable Product Multi Papaya" when the torrent name
            // (`Silicon.Valley.S01.…`) was the right answer.
            heal_tv_collection_identity(pool, &row.infohash).await;
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
        assign_after_ingest(
            pool,
            tmdb,
            providers,
            &row.infohash,
            &row.name,
            row.tmdb_id,
            &files,
        )
        .await;
        if row.tmdb_verified {
            enrich_after_verify(pool, &row.infohash).await;
        }
        done += 1;
    }
    if done > 0 {
        tracing::info!(count = done, "collection backfill complete");
    }
}
