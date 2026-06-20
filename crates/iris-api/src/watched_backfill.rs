//! Out-of-window watched-title backfill (RECOSYS.md §0 / WS2).
//!
//! The prod data showed only ~37% of watched titles live in the rolling-window
//! catalogue, so most of a user's history carried no embedding — and so couldn't
//! feed their taste profile. This pass fetches TMDB metadata for every
//! household-watched title that lacks a catalogue row and inserts it as a
//! metadata-only row (`availability='unknown'`). The reco embedding loop then
//! embeds it, so it becomes a profile positive. These rows are never recommended
//! back (the user already saw them) — they exist purely to sharpen centroids.

use std::time::Duration;

use iris_db::SqlitePool;

use crate::anilist::AniListClient;
use crate::freshness_scheduler::{is_anime_meta, pick_anime};
use crate::tmdb::{TmdbClient, TmdbKind};

/// Run one backfill pass. Best-effort: a failed lookup/upsert skips that title.
pub async fn run(pool: &SqlitePool, tmdb: &TmdbClient, anilist: Option<&AniListClient>) {
    let missing = match iris_db::catalog::watched_tmdbs_missing(pool).await {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(error = %e, "watched backfill: query failed");
            return;
        }
    };
    if missing.is_empty() {
        return;
    }

    let mut created = 0usize;
    for (tmdb_id, is_tv) in missing {
        let Ok(id) = u64::try_from(tmdb_id) else {
            continue;
        };
        let hint = if is_tv { TmdbKind::Tv } else { TmdbKind::Movie };
        let Some(m) = tmdb.lookup_with_kind(id, Some(hint)).await else {
            continue;
        };

        // Anime correlation — identical to the freshness scheduler so a watched
        // anime lands in the SAME dedup space (keyed on anilist_id), never as a
        // stray non-anime row. No AniList match → it stays a plain row.
        let anime = if is_anime_meta(&m) {
            match anilist {
                Some(a) => pick_anime(&a.search(&m.title).await, m.year),
                None => None,
            }
        } else {
            None
        };

        let mut item = iris_db::catalog::NewCatalogItem {
            tmdb_id: Some(tmdb_id),
            anilist_id: None,
            kind: match m.kind {
                TmdbKind::Movie => "movie",
                TmdbKind::Tv => "tv",
            }
            .to_string(),
            title: m.title.clone(),
            original_language: m.original_language.clone(),
            genres: m.genre_ids.iter().map(|&g| i64::from(g)).collect(),
            is_anime: false,
            poster_path: m.poster_path.clone(),
            backdrop_path: m.backdrop_path.clone(),
            overview: m.overview.clone(),
            popularity: m.popularity,
            vote_average: m.vote_score,
            release_date: m.release_date.clone(),
            source: Some("reco:watched-backfill".to_string()),
            // Metadata-only: resolved on click like any lazy reco candidate.
            availability: "unknown".to_string(),
            seeders: None,
            provider_id: None,
            external_id: None,
            download_url: None,
            infohash: None,
            language: None,
            released_at: None,
            size_bytes: None,
        };

        let res = if let Some(am) = anime {
            item.is_anime = true;
            item.anilist_id = Some(am.anilist_id);
            item.poster_path = am.cover_image.or_else(|| item.poster_path.take());
            item.backdrop_path = am.banner_image.or_else(|| item.backdrop_path.take());
            iris_db::catalog::upsert_anime(pool, &item).await
        } else {
            iris_db::catalog::upsert_item(pool, &item).await
        };
        match res {
            Ok(()) => created += 1,
            Err(e) => tracing::warn!(error = %e, tmdb_id, "watched backfill: upsert failed"),
        }
    }
    if created > 0 {
        tracing::info!(created, "watched backfill: added out-of-window titles");
    }
}

/// Spawn the backfill: once after a boot warm-up, then on a slow timer so titles
/// watched later (and outside the window) get added over time. Best-effort.
pub fn spawn(pool: SqlitePool, tmdb: TmdbClient) {
    tokio::spawn(async move {
        // Keyless AniList client for anime correlation (same as the scheduler);
        // absence just leaves anime as plain rows.
        let anilist = AniListClient::new().ok();
        tokio::time::sleep(Duration::from_secs(20)).await; // boot warm-up
        loop {
            run(&pool, &tmdb, anilist.as_ref()).await;
            tokio::time::sleep(Duration::from_hours(6)).await;
        }
    });
}
