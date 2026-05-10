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
    let mut renamed = 0u64;
    let mut unresolved = 0u64;
    for c in cols {
        // Re-derive everything from a member torrent's filename rather
        // than trusting the stored `parsed_title_normalized` /
        // `display_title` — both columns may be polluted by an older
        // parser that didn't recognise season packs and leaked tokens
        // like `S01 MULTI` into the title. Same logic as the web /
        // TV search-page client-side: extract the SCENE title from
        // the filename, hit TMDB.
        let torrents = match iris_db::torrents::list_in_collection(pool, c.id).await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(error = %e, collection_id = %c.id, "tmdb_backfill: list_in_collection failed");
                continue;
            }
        };
        let kind_hint = match c.kind.as_str() {
            "tv" => Some(crate::tmdb::TmdbKind::Tv),
            "movie" => Some(crate::tmdb::TmdbKind::Movie),
            _ => None,
        };
        // Walk the torrents in the collection until one resolves —
        // siblings sometimes have weirdly-shaped filenames and we want
        // any clean signal we can get. The first TMDB hit becomes the
        // canonical answer for the collection.
        let mut resolved_id: Option<i64> = None;
        let mut clean_display_title: Option<String> = None;
        for t in &torrents {
            // Re-parse with the live (post-fix) parser to derive a
            // clean title. We capture the parsed title for
            // `display_title` AND feed `resolve_release_name` to hit
            // TMDB on the cleaned form.
            let Some(parsed) = iris_media::filename::parse(&t.name) else { continue };
            if clean_display_title.is_none() {
                clean_display_title = Some(parsed.display_with_year(parsed.is_tv()));
            }
            if let Some(r) = crate::tmdb_resolve::resolve_release_name(
                pool, tmdb, &t.name, kind_hint,
            ).await {
                if let Ok(id) = i64::try_from(r.tmdb_id) {
                    resolved_id = Some(id);
                    // Prefer the TMDB-canonical title for display
                    // when we got one — it's typically prettier than
                    // the SCENE-derived form (real punctuation, real
                    // capitalisation). Falls back to the parsed
                    // SCENE title when TMDB doesn't return one.
                    if !r.title.trim().is_empty() {
                        clean_display_title = Some(r.title.clone());
                    }
                    break;
                }
            }
        }

        // Repair the display_title even when TMDB resolution fails.
        // The user-visible name flowing from the SCENE parser is at
        // least as clean as the polluted stored value, and matters
        // independently of poster lookup.
        if let Some(new_title) = clean_display_title.as_ref() {
            if !new_title.is_empty() && new_title != &c.display_title {
                if let Err(e) =
                    iris_db::collections::set_display_title(pool, c.id, new_title).await
                {
                    tracing::warn!(error = %e, collection_id = %c.id, "tmdb_backfill: set_display_title failed");
                } else {
                    renamed += 1;
                    tracing::info!(
                        collection_id = %c.id,
                        old = %c.display_title,
                        new = %new_title,
                        "tmdb_backfill: collection.display_title rewritten"
                    );
                }
            }
        }

        let Some(new_id) = resolved_id else {
            unresolved += 1;
            tracing::info!(
                collection_id = %c.id,
                display_title = %c.display_title,
                kind = %c.kind,
                current_tmdb_id = ?c.tmdb_id,
                torrents = torrents.len(),
                "tmdb_backfill: collection unresolved (no torrent matched TMDB)"
            );
            continue;
        };
        if c.tmdb_id == Some(new_id) {
            continue; // already correct
        }
        if let Err(e) = iris_db::collections::set_tmdb_id(pool, c.id, new_id).await {
            tracing::warn!(error = %e, collection_id = %c.id, "tmdb_backfill: collection set_tmdb_id failed");
            continue;
        }
        stamped += 1;
        tracing::info!(
            collection_id = %c.id,
            display_title = %clean_display_title.as_deref().unwrap_or(c.display_title.as_str()),
            kind = %c.kind,
            old_tmdb_id = ?c.tmdb_id,
            new_tmdb_id = new_id,
            "tmdb_backfill: collection.tmdb_id corrected"
        );
    }
    if stamped > 0 || renamed > 0 || unresolved > 0 {
        tracing::info!(
            stamped,
            renamed,
            unresolved,
            "tmdb_backfill: collection pass complete"
        );
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
            // Already on the right id and already verified — nothing to
            // do for this torrent.
            continue;
        }
        // DO NOT write to `collection.tmdb_id` here. The collection
        // owns its own resolution via `backfill_collections` (which
        // queries TMDB once with the collection's canonical
        // `parsed_title_normalized`, the way the web search works
        // client-side). A per-torrent stamp would race for "last
        // writer wins" against sibling torrents and clobber the
        // canonical value.
        //
        // Re-verify the (possibly new) tmdb_id against the file's
        // probed runtime. `try_verify` is best-effort — if the file
        // isn't on disk yet (still downloading) it short-circuits
        // and the next backfill pass picks it up. It only updates
        // `torrent.tmdb_verified`; `enrich_after_verify` inside
        // uses `set_tmdb_id_if_missing` which is a no-op once the
        // collection slot is filled by `backfill_collections`.
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
