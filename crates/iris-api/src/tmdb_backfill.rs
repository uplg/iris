//! One-shot collection TMDB id backfill.
//!
//! Mirrors the per-torrent SCENE resolver:
//!   1. Take a member torrent's filename.
//!   2. Run `iris_media::filename::parse` to extract `(title, year)`.
//!   3. `multi_search(title)` → score candidates by `(kind, year)` via
//!      [`crate::tmdb_resolve::pick_best`] and take the best match.
//!   4. Write its `tmdb_id` to the collection.
//!
//! Kind hint comes from the collection itself (`tv` vs `movie` already
//! decided at ingest). Year hint comes from the SCENE-parsed filename.
//! Both are critical: TMDB's `multi_search` orders by popularity, so a
//! query like `"Transformers"` returns the 1986 animated series (id
//! 4269) above the 2007 film (id 1858). Without the year filter, every
//! Transformers (200X) collection would inherit the cartoon's poster
//! despite the per-torrent path getting it right.

use std::time::Duration;

use crate::state::AppState;
use crate::tmdb::TmdbKind;
use crate::tmdb_resolve::pick_best;

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

enum CollectionOutcome {
    Stamped,
    AlreadyCorrect,
    NoTorrents,
    NoParse,
    NoHits,
    Error,
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
        match process_one_collection(pool, tmdb, &c).await {
            CollectionOutcome::Stamped => stamped += 1,
            CollectionOutcome::AlreadyCorrect => already_correct += 1,
            CollectionOutcome::NoTorrents => no_torrents += 1,
            CollectionOutcome::NoParse => no_parse += 1,
            CollectionOutcome::NoHits => no_hits += 1,
            CollectionOutcome::Error => {}
        }
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

async fn process_one_collection(
    pool: &iris_db::SqlitePool,
    tmdb: &crate::tmdb::TmdbClient,
    c: &iris_db::collections::CollectionRow,
) -> CollectionOutcome {
    let torrents = match iris_db::torrents::list_in_collection(pool, c.id).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(error = %e, collection_id = %c.id, "tmdb_backfill: list_in_collection failed");
            return CollectionOutcome::Error;
        }
    };
    if torrents.is_empty() {
        tracing::debug!(
            collection_id = %c.id,
            display_title = %c.display_title,
            "tmdb_backfill: SKIP — collection has no active torrents"
        );
        return CollectionOutcome::NoTorrents;
    }
    let Some((rep_name, parsed)) = torrents
        .iter()
        .find_map(|t| iris_media::filename::parse(&t.name).map(|p| (t.name.clone(), p)))
    else {
        tracing::debug!(
            collection_id = %c.id,
            display_title = %c.display_title,
            first_torrent = %torrents[0].name,
            "tmdb_backfill: SKIP — no torrent in collection parsed"
        );
        return CollectionOutcome::NoParse;
    };
    let title = parsed.title;
    let year_hint: Option<u32> = parsed.year.map(u32::from);
    let kind_hint: Option<TmdbKind> = match c.kind.as_str() {
        "movie" => Some(TmdbKind::Movie),
        "tv" => Some(TmdbKind::Tv),
        _ => None,
    };
    if title.trim().len() < 2 {
        tracing::debug!(
            collection_id = %c.id,
            display_title = %c.display_title,
            rep_name = %rep_name,
            "tmdb_backfill: SKIP — parsed title too short"
        );
        return CollectionOutcome::NoParse;
    }

    let hits = tmdb.multi_search(&title).await;
    // Score by `(kind, year)` instead of taking the popularity-sorted
    // top hit. Same logic the per-torrent SCENE resolver uses, so
    // collection-level and torrent-level tmdb_ids can't disagree on
    // the obvious year-disambiguated cases like Transformers 2007 vs
    // the 1986 animated series.
    let Some(top) = pick_best(&hits, kind_hint, year_hint) else {
        tracing::debug!(
            collection_id = %c.id,
            display_title = %c.display_title,
            rep_name = %rep_name,
            query = %title,
            year_hint,
            kind_hint = ?kind_hint,
            "tmdb_backfill: SKIP — no candidate matched kind/year"
        );
        return CollectionOutcome::NoHits;
    };
    let Ok(new_id) = i64::try_from(top.tmdb_id) else {
        return CollectionOutcome::Error;
    };
    if c.tmdb_id == Some(new_id) {
        tracing::debug!(
            collection_id = %c.id,
            display_title = %c.display_title,
            rep_name = %rep_name,
            query = %title,
            tmdb_id = new_id,
            top_title = %top.title,
            "tmdb_backfill: KEEP — TMDB top hit matches existing tmdb_id"
        );
        return CollectionOutcome::AlreadyCorrect;
    }
    if let Err(e) = iris_db::collections::set_tmdb_id(pool, c.id, new_id).await {
        tracing::warn!(error = %e, collection_id = %c.id, "tmdb_backfill: set_tmdb_id failed");
        return CollectionOutcome::Error;
    }
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
    CollectionOutcome::Stamped
}
