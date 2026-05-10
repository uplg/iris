//! One-shot migration pass that re-resolves `torrents.tmdb_id` from
//! the SCENE-cleaned release name.
//!
//! Existing rows ingested before the override flow landed in
//! `routes/torrents.rs::ingest` carry the indexer's (often wrong)
//! `tmdb_id` from search-time. This pass walks them once at boot and
//! overwrites the column with what the SCENE name actually points
//! at, using the same persistent cache the live ingestion path uses.
//!
//! Not a recurring task: the live ingestion override means *new*
//! torrents already arrive with the correct tmdb_id, and the existing
//! `play_asset` route runs the runtime probe whenever a user plays a
//! file. The only thing left to handle is the historical backlog,
//! which is a migration problem — once swept, there's nothing to do.

use std::time::Duration;

use crate::state::AppState;
use crate::tmdb_resolve;

/// Same threshold the live ingestion / play paths use. Defined here
/// (and not pulled from `routes::torrents`) so the backfill can
/// re-validate without crossing module boundaries.
const TMDB_RUNTIME_TOLERANCE: f64 = 0.15;

/// Spawn the boot-time migration. No-op if TMDB isn't configured.
/// Runs once after a short delay so it doesn't fight the
/// collection-assignment backfill for the same DB lock; nothing
/// schedules a follow-up pass.
pub fn spawn(state: AppState) {
    if state.tmdb().is_none() {
        return;
    }
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(45)).await;
        run_once(&state).await;
    });
}

async fn run_once(state: &AppState) {
    // Two passes:
    //   1. Collection-level — resolve `tmdb_id` from each collection's
    //      own canonical SCENE-normalised name. This is what the
    //      library / Continue Watching cards read, and resolving from
    //      the collection identity (not from an arbitrary member
    //      torrent's filename) means weirdly-named individual
    //      torrents can't poison the collection's poster.
    //   2. Per-torrent — keep `torrents.tmdb_id` in sync for surfaces
    //      that read it directly (TorrentDetails, single-torrent
    //      views), and drive `tmdb_verified` via the runtime probe.
    backfill_collections(state).await;
    backfill_torrents(state).await;
}

async fn backfill_collections(state: &AppState) {
    let pool = state.db();
    let Some(tmdb) = state.tmdb() else { return };
    let cols = match iris_db::collections::list_all(pool).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "tmdb_backfill: list_all collections failed");
            return;
        }
    };
    let mut stamped = 0u64;
    let mut unresolved = 0u64;
    for c in cols {
        let Some(cleaned) = c.parsed_title_normalized.as_ref() else {
            // Standalone (no SCENE parse) — nothing to look up against.
            continue;
        };
        if cleaned.len() < 2 {
            continue;
        }
        let kind_hint = match c.kind.as_str() {
            "tv" => Some(crate::tmdb::TmdbKind::Tv),
            "movie" => Some(crate::tmdb::TmdbKind::Movie),
            _ => None,
        };
        // For movies the normalised key carries a "title YYYY" suffix
        // (`Parsed::collection_key`); split it back out so multi_search
        // hits TMDB on a clean title and we can use the year as a
        // disambiguator. TV keys never have a year suffix.
        let (query, year_hint) = if kind_hint == Some(crate::tmdb::TmdbKind::Movie) {
            split_year_suffix(cleaned)
        } else {
            (cleaned.as_str(), None)
        };
        let resolved = crate::tmdb_resolve::resolve_cleaned(
            pool,
            tmdb,
            query,
            kind_hint,
            year_hint,
        )
        .await;
        let Some(r) = resolved else {
            unresolved += 1;
            tracing::info!(
                collection_id = %c.id,
                display_title = %c.display_title,
                cleaned = %cleaned,
                kind = %c.kind,
                current_tmdb_id = ?c.tmdb_id,
                "tmdb_backfill: collection unresolved (no confident TMDB match)"
            );
            continue;
        };
        let resolved_id = match i64::try_from(r.tmdb_id) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if c.tmdb_id == Some(resolved_id) {
            continue; // already correct, no work needed
        }
        if let Err(e) = iris_db::collections::set_tmdb_id(pool, c.id, resolved_id).await {
            tracing::warn!(error = %e, collection_id = %c.id, "tmdb_backfill: collection write failed");
            continue;
        }
        stamped += 1;
        tracing::info!(
            collection_id = %c.id,
            display_title = %c.display_title,
            kind = %c.kind,
            old_tmdb_id = ?c.tmdb_id,
            new_tmdb_id = resolved_id,
            "tmdb_backfill: collection.tmdb_id corrected"
        );
    }
    if stamped > 0 || unresolved > 0 {
        tracing::info!(
            stamped,
            unresolved,
            "tmdb_backfill: collection pass complete"
        );
    }
}

/// `Parsed::collection_key(false)` produces `"title YYYY"` for movies
/// (e.g., `"dune 1984"`). Split the trailing 4-digit year back off so
/// TMDB's multi-search gets a clean query and we can use the year as
/// a tie-breaker. Returns `(title, None)` when there's no year suffix.
fn split_year_suffix(key: &str) -> (&str, Option<u32>) {
    let bytes = key.as_bytes();
    let n = bytes.len();
    if n < 5 || bytes[n - 5] != b' ' {
        return (key, None);
    }
    let tail = &bytes[n - 4..];
    if !tail.iter().all(u8::is_ascii_digit) {
        return (key, None);
    }
    let Ok(year_str) = std::str::from_utf8(tail) else {
        return (key, None);
    };
    let Ok(year) = year_str.parse::<u32>() else {
        return (key, None);
    };
    if (1900..=2099).contains(&year) {
        (&key[..n - 5], Some(year))
    } else {
        (key, None)
    }
}

async fn backfill_torrents(state: &AppState) {
    let pool = state.db();
    let Some(tmdb) = state.tmdb() else { return };
    let rows = match iris_db::torrents::list_active(pool).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "tmdb_backfill: list_active failed");
            return;
        }
    };
    let mut changed = 0u64;
    let mut verified = 0u64;
    for row in rows {
        let resolved = tmdb_resolve::resolve_release_name(pool, tmdb, &row.name, None).await;
        let Some(r) = resolved else { continue };
        let Ok(resolved_id) = i64::try_from(r.tmdb_id) else { continue };

        let id_changed = row.tmdb_id != Some(resolved_id);
        if id_changed {
            // Overwrite with the SCENE-resolved value AND drop
            // `tmdb_verified` — the verification flag was attached to
            // the OLD (now-replaced) tmdb_id; we re-verify below
            // against the new id using the cached runtime probe, so
            // the bit doesn't stay false on the way out unless we
            // actually fail to match.
            let res = sqlx::query(
                "UPDATE torrents SET tmdb_id = ?1, tmdb_verified = 0 \
                 WHERE infohash = ?2",
            )
            .bind(resolved_id)
            .bind(&row.infohash)
            .execute(pool)
            .await;
            match res {
                Ok(rr) if rr.rows_affected() > 0 => {
                    changed += 1;
                    tracing::info!(
                        infohash = %row.infohash,
                        name = %row.name,
                        old = ?row.tmdb_id,
                        new = resolved_id,
                        "tmdb_backfill: corrected torrent tmdb_id"
                    );
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(error = %e, infohash = %row.infohash, "tmdb_backfill: torrent update failed");
                    continue;
                }
            }
        } else if row.tmdb_verified {
            // Already on the right id and already verified — skip the
            // collection write (no-op) and the verify probe.
            continue;
        }
        // Propagate to the parent collection — UNCONDITIONALLY when we
        // got a SCENE-resolved id, even when the torrent's own id
        // didn't change. Reason: `collection_assign::run_backfill`
        // (which runs *before* this task at boot) already stamped
        // every collection with whatever was on the torrent at that
        // point, *including* the indexer's wrong values. If the SCENE
        // resolution agrees with what's already on the torrent
        // (id_changed=false) but the collection was stamped from a
        // sibling torrent that had a wrong id, the slot would stay
        // wrong forever. The write is idempotent when collection
        // already has the right id.
        if let Some(collection_id) = row.collection_id {
            match iris_db::collections::set_tmdb_id(pool, collection_id, resolved_id).await {
                Ok(_) => {
                    tracing::info!(
                        infohash = %row.infohash,
                        collection_id = %collection_id,
                        tmdb_id = resolved_id,
                        torrent_changed = id_changed,
                        "tmdb_backfill: stamped collection.tmdb_id",
                    );
                }
                Err(e) => tracing::warn!(
                    error = %e,
                    infohash = %row.infohash,
                    collection_id = %collection_id,
                    "tmdb_backfill: collection set_tmdb_id failed",
                ),
            }
        }
        // Re-verify the (possibly new) tmdb_id against the file's
        // probed runtime. `try_verify` is best-effort — if the file
        // isn't on disk yet (still downloading) it short-circuits
        // and the next backfill pass picks it up.
        if try_verify(state, &row.infohash, resolved_id).await {
            verified += 1;
        }
    }
    if changed > 0 || verified > 0 {
        tracing::info!(
            corrected = changed,
            verified,
            "tmdb_backfill: pass complete"
        );
    }
}

/// Probe + TMDB runtime check for `(infohash, tmdb_id)`. Mirrors the
/// shape of `routes::torrents::verify_tmdb_match` but keyed on a
/// caller-supplied tmdb_id rather than re-reading the row, since the
/// backfill just wrote a new value and a fresh `find_by_infohash`
/// would race with itself. Picks the largest video file in the
/// torrent as the probe target — same heuristic as `prewarm_default_remux`.
async fn try_verify(state: &AppState, infohash: &str, tmdb_id: i64) -> bool {
    let Ok(tmdb_id_u64) = u64::try_from(tmdb_id) else { return false };
    let Some(snap) = state.engine().get_by_infohash(infohash) else { return false };
    if !snap.finished {
        return false;
    }
    let video_exts = ["mkv", "mp4", "webm", "m4v", "avi", "mov", "ts", "mts", "m2ts", "wmv"];
    let target_idx = snap
        .files
        .iter()
        .filter(|f| {
            let p = std::path::Path::new(&f.path);
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| video_exts.iter().any(|ext| ext.eq_ignore_ascii_case(e)))
        })
        .max_by_key(|f| f.size_bytes)
        .map(|f| f.index);
    let Some(idx) = target_idx else { return false };
    let Ok(path) = state.engine().file_path(infohash, idx) else { return false };
    if !path.exists() {
        return false;
    }
    let probed = match state.probes().get_or_probe(infohash, idx, &path).await {
        Ok(p) => p.duration_seconds,
        Err(e) => {
            tracing::debug!(error = %e, infohash, "tmdb_backfill: probe failed");
            return false;
        }
    };
    let Some(probed_secs) = probed.filter(|d| *d > 0.0) else { return false };
    let Some(tmdb) = state.tmdb() else { return false };
    let Some(meta) = tmdb.lookup(tmdb_id_u64).await else { return false };
    let Some(tmdb_minutes) = meta.runtime_minutes.filter(|m| *m > 0) else { return false };
    let tmdb_secs = f64::from(tmdb_minutes) * 60.0;
    let diff = (probed_secs - tmdb_secs).abs() / tmdb_secs;
    let verified = diff < TMDB_RUNTIME_TOLERANCE;
    if let Err(e) =
        iris_db::torrents::set_tmdb_verified(state.db(), infohash, verified).await
    {
        tracing::warn!(error = %e, infohash, "tmdb_backfill: set_tmdb_verified failed");
        return false;
    }
    if verified {
        // The collection's `tmdb_id` was already force-propagated up
        // in `run_once` when the torrent's id was rewritten — no need
        // to repeat it here. `enrich_after_verify` would no-op
        // anyway against the now-correct collection slot, but call
        // it for symmetry with the live ingestion flow (a future
        // refactor that adds side-effects to `enrich_after_verify`
        // will see backfilled torrents too).
        crate::collection_assign::enrich_after_verify(state.db(), infohash).await;
        tracing::info!(
            infohash,
            tmdb_id,
            probed_secs,
            tmdb_secs,
            "tmdb_backfill: verified"
        );
    } else {
        tracing::info!(
            infohash,
            tmdb_id,
            probed_secs,
            tmdb_secs,
            diff_pct = diff * 100.0,
            "tmdb_backfill: still unverified after re-resolve"
        );
    }
    verified
}
