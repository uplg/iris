//! One-shot collection TMDB id backfill.
//!
//! Mirrors what the web search dropdown does:
//!   1. Take a member torrent's filename.
//!   2. Run `iris_media::filename::parse` to extract the SCENE title.
//!   3. `multi_search(title)` → take the first hit.
//!   4. Write its `tmdb_id` to the collection.
//!
//! No kind filter. No year filter. No preservation. No display_title
//! rewrite. No fallback to torr9's tmdb_id. The first TMDB hit wins,
//! every time, exactly like the web search suggestion.

use std::time::Duration;

use crate::state::AppState;

/// One-shot at boot. Runs after a 45 s delay so it doesn't fight the
/// collection-assignment backfill for the same DB lock; nothing
/// schedules a follow-up pass.
pub fn spawn(state: AppState) {
    if state.tmdb().is_none() {
        return;
    }
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(45)).await;
        run_collections(&state).await;
    });
}

async fn run_collections(state: &AppState) {
    let pool = state.db();
    let Some(tmdb) = state.tmdb() else { return };

    let cols = match iris_db::collections::list_all(pool).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "tmdb_backfill: list_all collections failed");
            return;
        }
    };
    tracing::info!(total = cols.len(), "tmdb_backfill: starting collection pass");

    let mut stamped = 0u64;
    let mut already_correct = 0u64;
    let mut no_torrents = 0u64;
    let mut no_parse = 0u64;
    let mut no_hits = 0u64;
    for c in cols {
        let torrents = match iris_db::torrents::list_in_collection(pool, c.id).await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(error = %e, collection_id = %c.id, "tmdb_backfill: list_in_collection failed");
                continue;
            }
        };
        if torrents.is_empty() {
            no_torrents += 1;
            tracing::info!(
                collection_id = %c.id,
                display_title = %c.display_title,
                "tmdb_backfill: SKIP — collection has no active torrents"
            );
            continue;
        }
        let Some((rep_name, title)) = torrents
            .iter()
            .find_map(|t| iris_media::filename::parse(&t.name).map(|p| (t.name.clone(), p.title)))
        else {
            no_parse += 1;
            tracing::info!(
                collection_id = %c.id,
                display_title = %c.display_title,
                first_torrent = %torrents[0].name,
                "tmdb_backfill: SKIP — no torrent in collection parsed"
            );
            continue;
        };
        if title.trim().len() < 2 {
            no_parse += 1;
            tracing::info!(
                collection_id = %c.id,
                display_title = %c.display_title,
                rep_name = %rep_name,
                "tmdb_backfill: SKIP — parsed title too short"
            );
            continue;
        }

        let hits = tmdb.multi_search(&title).await;
        let Some(top) = hits.first() else {
            no_hits += 1;
            tracing::info!(
                collection_id = %c.id,
                display_title = %c.display_title,
                rep_name = %rep_name,
                query = %title,
                "tmdb_backfill: SKIP — TMDB returned no hits"
            );
            continue;
        };
        let Ok(new_id) = i64::try_from(top.tmdb_id) else { continue };
        if c.tmdb_id == Some(new_id) {
            already_correct += 1;
            tracing::info!(
                collection_id = %c.id,
                display_title = %c.display_title,
                rep_name = %rep_name,
                query = %title,
                tmdb_id = new_id,
                top_title = %top.title,
                "tmdb_backfill: KEEP — TMDB top hit matches existing tmdb_id"
            );
            continue;
        }
        if let Err(e) = iris_db::collections::set_tmdb_id(pool, c.id, new_id).await {
            tracing::warn!(error = %e, collection_id = %c.id, "tmdb_backfill: set_tmdb_id failed");
            continue;
        }
        stamped += 1;
        tracing::info!(
            collection_id = %c.id,
            display_title = %c.display_title,
            rep_name = %rep_name,
            query = %title,
            old_tmdb_id = ?c.tmdb_id,
            new_tmdb_id = new_id,
            top_title = %top.title,
            "tmdb_backfill: WRITE — collection.tmdb_id corrected"
        );
    }
    tracing::info!(
        stamped,
        already_correct,
        no_torrents,
        no_parse,
        no_hits,
        "tmdb_backfill: collection pass complete"
    );
}
