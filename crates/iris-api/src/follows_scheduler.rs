// Season / episode / seeders / size casts move between i64 and
// u32/u64 — domain values are positive and bounded, so pedantic
// cast warnings are noise.
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
)]

//! Notify-only scheduler for `series_follows`.
//!
//! Wakes every 4 hours and walks each user's follows. For every
//! follow, asks the indexer for everything matching the SCENE name
//! and stashes new (S, E) hits into `available_episodes` so the
//! eventual user click on "Préparer" doesn't gate on a fresh
//! indexer query.
//!
//! Pure SCENE pipeline — no TMDB call, no expected-episode grid.
//! "What episodes exist" is whatever the indexer returns when
//! searching the show's name. Episodes the indexer doesn't know
//! about don't appear; that's correct because Iris can't grab
//! something the indexer doesn't list anyway.
//!
//! Crucially this scheduler **never** triggers an ingest. Auto-grab
//! was explicitly rejected at planning time — the user wants to be
//! notified and stay in control of disk usage. The on-demand grab
//! endpoint (`/api/me/follows/:id/episodes/:s/:e/grab`) is what
//! actually pulls a torrent.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use iris_core::search::{SearchQuery, SearchResult, SortField, SortOrder};
use iris_db::SqlitePool;
use iris_media::filename;
use iris_providers::ProviderRegistry;

/// How often to scan all follows. Most series ship a new episode
/// once a week, so 4h means we surface a release within hours.
const TICK_INTERVAL: Duration = Duration::from_secs(4 * 60 * 60);

/// Skip per-follow work if `last_checked_at` is younger than this.
const PER_FOLLOW_COOLDOWN: Duration = Duration::from_secs(2 * 60 * 60);

pub fn spawn(pool: SqlitePool, providers: ProviderRegistry) {
    let providers = Arc::new(providers);
    tokio::spawn(async move {
        // Initial pass after a short warm-up so existing follows
        // get their `available_episodes` populated within seconds
        // of boot, not at the 4 h mark.
        tokio::time::sleep(Duration::from_secs(10)).await;
        run_pass(&pool, providers.clone()).await;

        let mut ticker = tokio::time::interval(TICK_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        ticker.tick().await; // skip the immediate firing
        loop {
            ticker.tick().await;
            run_pass(&pool, providers.clone()).await;
        }
    });
    tracing::info!(
        interval_secs = TICK_INTERVAL.as_secs(),
        "follows notify scheduler started"
    );
}

async fn run_pass(pool: &SqlitePool, providers: Arc<ProviderRegistry>) {
    let follows = match all_follows_due(pool).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "scheduler: list follows failed");
            return;
        }
    };
    if follows.is_empty() {
        return;
    }
    tracing::info!(count = follows.len(), "scheduler: scanning follows");
    for follow in follows {
        if let Err(e) = check_one(pool, &providers, &follow).await {
            tracing::warn!(
                follow_id = %follow.id,
                name = %follow.name,
                error = %e,
                "scheduler: follow check failed",
            );
        }
        let _ = iris_db::follows::mark_checked(pool, follow.id).await;
    }
}

async fn all_follows_due(
    pool: &SqlitePool,
) -> Result<Vec<iris_db::follows::FollowRow>, sqlx::Error> {
    let cutoff = Utc::now() - chrono::Duration::from_std(PER_FOLLOW_COOLDOWN).unwrap();
    sqlx::query_as::<_, iris_db::follows::FollowRow>(
        "SELECT id, user_id, normalized_name, name, tmdb_id, last_checked_at, \
                last_visited_at, created_at \
         FROM series_follows \
         WHERE last_checked_at IS NULL OR last_checked_at < ?1 \
         ORDER BY last_checked_at ASC NULLS FIRST",
    )
    .bind(cutoff)
    .fetch_all(pool)
    .await
}

/// Public entry-point — scan one follow. Used by the scheduler
/// tick AND by the create-follow route so a freshly followed
/// series shows `dispo` chips on first visit.
pub async fn scan_follow(
    pool: &SqlitePool,
    providers: &ProviderRegistry,
    follow: &iris_db::follows::FollowRow,
) -> anyhow::Result<()> {
    check_one(pool, providers, follow).await
}

async fn check_one(
    pool: &SqlitePool,
    providers: &ProviderRegistry,
    follow: &iris_db::follows::FollowRow,
) -> anyhow::Result<()> {
    // Single broad search per follow — `<show name>`. The indexer
    // returns whatever episodes / packs / specials it has; we
    // SCENE-parse each title to extract (S, E) and dedup.
    let q = SearchQuery {
        q: follow.name.clone(),
        page: Some(1),
        limit: Some(100),
        sort_by: Some(SortField::Seeders),
        order: Some(SortOrder::Desc),
        kind: None,
    };
    let agg = providers.search_all(&q).await;
    if agg.results.is_empty() {
        return Ok(());
    }

    // Group results by (S, E); within each group keep the
    // best-seeded entry. Filtering on normalized title match keeps
    // unrelated indexer noise (e.g., "Squid Game Documentary") out.
    let mut best: std::collections::HashMap<(i64, i64), SearchResult> =
        std::collections::HashMap::new();
    for r in agg.results {
        let Some(parsed) = filename::parse(&r.title) else {
            continue;
        };
        // Use the TV-side `collection_key` so a SCENE name like
        // `Lucky.Luke.1991.S01E01...` (which normalises to
        // `"lucky luke 1991"`) still matches a follow whose
        // `normalized_name` was derived from the user-typed
        // `"Lucky Luke"` (= `"lucky luke"`).
        if parsed.collection_key(true) != follow.normalized_name {
            continue;
        }
        let Some(s) = parsed.season else { continue };
        let Some(e) = parsed.episode else { continue };
        let key = (i64::from(s), i64::from(e));
        let cur_seeders = r.seeders.unwrap_or(0);
        match best.get(&key) {
            Some(existing) if existing.seeders.unwrap_or(0) >= cur_seeders => {}
            _ => {
                best.insert(key, r);
            }
        }
    }

    for ((season, episode), result) in best {
        // Skip if we've already cached this exact (provider × torrent_id) —
        // upsert refreshes seeders / found_at but we'd waste a write
        // if nothing changed. Cheap COUNT here.
        if availability_exists(pool, &follow.normalized_name, season, episode).await? {
            continue;
        }
        record_availability(pool, providers, &follow.normalized_name, season, episode, &result)
            .await;
    }
    Ok(())
}

async fn availability_exists(
    pool: &SqlitePool,
    normalized_name: &str,
    season: i64,
    episode: i64,
) -> Result<bool, sqlx::Error> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM available_episodes \
         WHERE normalized_name = ?1 AND season = ?2 AND episode = ?3",
    )
    .bind(normalized_name)
    .bind(season)
    .bind(episode)
    .fetch_one(pool)
    .await?;
    Ok(row.0 > 0)
}

async fn record_availability(
    pool: &SqlitePool,
    providers: &ProviderRegistry,
    normalized_name: &str,
    season: i64,
    episode: i64,
    best: &SearchResult,
) {
    if !providers.ids().iter().any(|id| id == &best.provider_id) {
        return;
    }
    let upsert = iris_db::available_episodes::UpsertAvailableEpisode {
        normalized_name: normalized_name.to_string(),
        season,
        episode,
        indexer_provider: best.provider_id.clone(),
        indexer_torrent_id: best.external_id.clone(),
        magnet: String::new(),
        quality: best
            .tags
            .iter()
            .find(|t| t.contains("1080p") || t.contains("720p") || t.contains("2160p"))
            .cloned()
            .or_else(|| extract_quality_from_title(&best.title)),
        seeders: best.seeders.map(i64::from),
        size_bytes: best.size_bytes.map(|s| s as i64),
    };
    if let Err(e) = iris_db::available_episodes::upsert(pool, upsert).await {
        tracing::warn!(error = %e, "scheduler: upsert availability failed");
    } else {
        tracing::info!(
            normalized_name, season, episode,
            provider = %best.provider_id,
            seeders = best.seeders.unwrap_or(0),
            "scheduler: cached new episode",
        );
    }
}

fn extract_quality_from_title(title: &str) -> Option<String> {
    for q in ["2160p", "1080p", "720p", "480p"] {
        if title.contains(q) {
            return Some(q.to_string());
        }
    }
    None
}
