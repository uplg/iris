//! Recommendation catalogue scheduler (discovery only).
//!
//! Every few hours, populate `catalog_items` from the *metadata* sources
//! the onboarded household wants: TMDB (trending + per-language discover +
//! now-playing / on-the-air) and AniList (anime, when anyone enabled it).
//!
//! It deliberately **never queries the torrent trackers**. Availability is
//! resolved lazily, only when a user actually clicks a recommendation (the
//! normal search → grab flow) — so a quiet home costs zero tracker
//! requests, and the trackers are never bursted into 429s. TMDB / AniList
//! are metadata APIs with generous limits and are cached.
//!
//! Only spawned when a TMDB client is configured.

use std::collections::{BTreeSet, HashSet};
use std::time::Duration;

use chrono::{Datelike, Utc};
use iris_db::SqlitePool;
use tokio::time::MissedTickBehavior;

use crate::anilist::{AniListClient, AniSeason};
use crate::reco::pref_to_iso639;
use crate::tmdb::{DiscoverParams, MediaMetadata, TmdbClient, TmdbKind};

const TICK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const PRUNE_AFTER_DAYS: i64 = 30;
/// `±` window around now for the AniList airing-schedule query.
const AIRING_WINDOW_DAYS: i64 = 7;
/// Only seed releases from the last N years — keep the catalogue fresh.
const RECENT_WINDOW_YEARS: i64 = 3;

pub fn spawn(pool: SqlitePool, tmdb: TmdbClient) {
    // Keyless — construct once; absence just disables the anime catalogue.
    let anilist = match AniListClient::new() {
        Ok(c) => Some(c),
        Err(e) => {
            tracing::warn!(error = %e, "anilist client init failed; anime catalogue disabled");
            None
        }
    };

    tokio::spawn(async move {
        // Small warm-up so the first pass doesn't contend with boot.
        tokio::time::sleep(Duration::from_secs(15)).await;
        run_pass(&pool, &tmdb, anilist.as_ref()).await;

        let mut ticker = tokio::time::interval(TICK_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        ticker.tick().await; // consume the immediate tick (already ran once)
        loop {
            ticker.tick().await;
            run_pass(&pool, &tmdb, anilist.as_ref()).await;
        }
    });
}

async fn run_pass(pool: &SqlitePool, tmdb: &TmdbClient, anilist: Option<&AniListClient>) {
    // Union of onboarded prefs → which language slices to fetch. Batching
    // by household demand keeps TMDB calls bounded regardless of user
    // count.
    let onboarded = match iris_db::preferences::all_onboarded(pool).await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "reco scheduler: loading prefs failed");
            return;
        }
    };
    let mut languages: BTreeSet<&'static str> = BTreeSet::new();
    for prefs in &onboarded {
        for lang in &prefs.languages {
            if let Some(iso) = pref_to_iso639(lang) {
                languages.insert(iso);
            }
        }
    }

    let mut upserted = 0usize;
    let today = Utc::now().format("%Y-%m-%d").to_string();
    // Recent window: a discovery home wants fresh content, not all-time
    // classics (TMDB popularity keeps timeless films like Léon near the
    // top forever). Only seed releases from the last few years.
    let recent_gte = (Utc::now() - chrono::Duration::days(365 * RECENT_WINDOW_YEARS))
        .format("%Y-%m-%d")
        .to_string();

    // Movies: discover by popularity within the recent window, then keep
    // only those actually out at home (a past digital/physical release) —
    // so a film still only in cinemas is never surfaced. Verification is
    // per-title against TMDB release dates (metadata API, cached), never a
    // tracker call.
    let movie_baseline = DiscoverParams {
        sort_by: Some("popularity.desc".to_string()),
        date_gte: Some(recent_gte.clone()),
        ..Default::default()
    };
    let movies = home_released(tmdb, tmdb.discover(TmdbKind::Movie, &movie_baseline).await, &today).await;
    upserted += store(pool, movies, "tmdb:discover:movie").await;

    // TV: an aired episode is grabbable same-day, so trending + on-the-air
    // are fine (no at-home-release gating).
    upserted += store(pool, tmdb.trending(TmdbKind::Tv).await, "tmdb:trending").await;
    upserted += store(pool, tmdb.on_the_air().await, "tmdb:on_the_air").await;

    // Per-language discover slices the household actually wants.
    for iso in &languages {
        let movie_params = DiscoverParams {
            original_language: Some((*iso).to_string()),
            sort_by: Some("popularity.desc".to_string()),
            date_gte: Some(recent_gte.clone()),
            ..Default::default()
        };
        let movies =
            home_released(tmdb, tmdb.discover(TmdbKind::Movie, &movie_params).await, &today).await;
        upserted += store(pool, movies, &format!("tmdb:discover:movie:{iso}")).await;

        let tv_params = DiscoverParams {
            original_language: Some((*iso).to_string()),
            sort_by: Some("popularity.desc".to_string()),
            date_gte: Some(recent_gte.clone()),
            ..Default::default()
        };
        upserted += store(
            pool,
            tmdb.discover(TmdbKind::Tv, &tv_params).await,
            &format!("tmdb:discover:tv:{iso}"),
        )
        .await;
    }

    // Anime catalogue (AniList) — only when at least one onboarded user
    // wants anime. Each title is reconciled to TMDB where possible and
    // kept AniList-only otherwise (never dropped).
    if onboarded.iter().any(|p| p.include_anime) {
        if let Some(anilist) = anilist {
            upserted += store_anime(pool, tmdb, anilist).await;
        }
    }

    let cutoff = Utc::now() - chrono::Duration::days(PRUNE_AFTER_DAYS);
    let pruned = iris_db::catalog::prune_stale(pool, cutoff).await.unwrap_or(0);
    tracing::info!(
        upserted,
        pruned,
        languages = languages.len(),
        "reco catalogue pass complete"
    );
}

/// Keep only movies that are out at home. Releases over a year old are
/// assumed available and pass without a per-title call; recent ones are
/// verified against TMDB release dates, so a theatrical-only new film
/// (still in cinemas, no digital/physical release yet) is dropped.
async fn home_released(
    tmdb: &TmdbClient,
    movies: Vec<MediaMetadata>,
    today: &str,
) -> Vec<MediaMetadata> {
    let this_year: i32 = today.get(..4).and_then(|y| y.parse().ok()).unwrap_or(0);
    let mut out = Vec::with_capacity(movies.len());
    for m in movies {
        // Only this year / last year are ambiguous (theatrical window);
        // older films are reliably out at home.
        let recent = m
            .year
            .is_none_or(|y| i32::try_from(y).unwrap_or(i32::MAX) >= this_year - 1);
        if !recent || tmdb.has_home_release(m.tmdb_id, today).await {
            out.push(m);
        }
    }
    out
}

/// Upsert a batch of TMDB candidates into the catalogue. TMDB-sourced rows
/// are never anime (`is_anime = false`) — the `AniList` pass owns that
/// dimension.
async fn store(pool: &SqlitePool, items: Vec<MediaMetadata>, source: &str) -> usize {
    let mut stored = 0;
    for m in items {
        // TMDB ids comfortably fit i64; skip the impossible rather than
        // wrap-cast (and trip clippy).
        let Ok(tmdb_id) = i64::try_from(m.tmdb_id) else {
            continue;
        };
        let item = iris_db::catalog::NewCatalogItem {
            tmdb_id: Some(tmdb_id),
            anilist_id: None,
            kind: match m.kind {
                TmdbKind::Movie => "movie",
                TmdbKind::Tv => "tv",
            }
            .to_string(),
            title: m.title,
            original_language: m.original_language,
            genres: m.genre_ids.iter().map(|&g| i64::from(g)).collect(),
            is_anime: false,
            poster_path: m.poster_path,
            backdrop_path: m.backdrop_path,
            overview: m.overview,
            popularity: m.popularity,
            vote_average: m.vote_score,
            release_date: m.release_date,
            source: Some(source.to_string()),
        };
        match iris_db::catalog::upsert_item(pool, &item).await {
            Ok(()) => stored += 1,
            Err(e) => tracing::warn!(error = %e, "reco scheduler: upsert failed"),
        }
    }
    stored
}

/// Fetch the `AniList` anime catalogue (seasonal + airing window),
/// reconcile each title to TMDB where possible, and upsert as anime rows.
/// AniList-only rows (no TMDB match) keep the `AniList` cover as poster —
/// a title is never dropped for lack of a TMDB match.
async fn store_anime(pool: &SqlitePool, tmdb: &TmdbClient, anilist: &AniListClient) -> usize {
    let now = Utc::now();
    let season = AniSeason::from_month(now.month());
    let mut media = anilist.seasonal(season, now.year()).await;
    let from = (now - chrono::Duration::days(AIRING_WINDOW_DAYS)).timestamp();
    let to = (now + chrono::Duration::days(AIRING_WINDOW_DAYS)).timestamp();
    media.extend(anilist.airing_window(from, to).await);

    let mut seen: HashSet<i64> = HashSet::new();
    let mut stored = 0usize;
    for m in media {
        if !seen.insert(m.anilist_id) {
            continue;
        }
        let kind_hint = if m.is_movie {
            TmdbKind::Movie
        } else {
            TmdbKind::Tv
        };
        // Reconcile to TMDB by title; fall back to AniList-only.
        let suggestions = tmdb.multi_search(&m.title).await;
        let best =
            crate::tmdb_resolve::pick_best(&suggestions, Some(kind_hint), m.year.map(u32::from));
        let (tmdb_id, kind, poster) = match best {
            Some(s) => (
                i64::try_from(s.tmdb_id).ok(),
                s.kind,
                s.poster_path.or_else(|| m.cover_image.clone()),
            ),
            None => (None, kind_hint, m.cover_image.clone()),
        };
        let item = iris_db::catalog::NewCatalogItem {
            tmdb_id,
            anilist_id: Some(m.anilist_id),
            kind: match kind {
                TmdbKind::Movie => "movie",
                TmdbKind::Tv => "tv",
            }
            .to_string(),
            title: m.title,
            original_language: Some("ja".to_string()),
            genres: Vec::new(),
            is_anime: true,
            poster_path: poster,
            backdrop_path: m.banner_image,
            overview: m.description,
            popularity: m.popularity,
            vote_average: m.average_score,
            release_date: m.release_date,
            source: Some("anilist".to_string()),
        };
        match iris_db::catalog::upsert_anime(pool, &item).await {
            Ok(()) => stored += 1,
            Err(e) => tracing::warn!(error = %e, "reco scheduler: anime upsert failed"),
        }
    }
    stored
}
