// `tmdb_id`, `season`, `episode`, `seeders`, `size_bytes` casts are all
// from positive bounded values (DB-stored as i64 / u64 but never negative,
// always within u32 range for the TMDB ones). Clippy's cast warnings here
// are pedantic noise.
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
)]

//! Notify-only scheduler for `series_follows`.
//!
//! Wakes every 4 hours, walks each user's follows, and asks TMDB for the
//! current season's episode list. Anything aired and not yet in
//! `episode_files` gets a single indexer search; the best result lands
//! in `available_episodes` so the user's eventual click on "Préparer" or
//! "Lire" goes through the fast path instead of waiting on a fresh
//! query.
//!
//! Crucially this scheduler **never** triggers an ingest. Auto-grab was
//! explicitly rejected at planning time — the user wants to be notified
//! and stay in control of disk usage. The on-demand grab endpoint
//! (`/api/me/follows/:tmdb_id/episodes/:s/:e/grab`) is what actually
//! pulls a torrent.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use iris_core::search::{SearchQuery, SearchResult, SortField, SortOrder};
use iris_db::SqlitePool;
use iris_providers::ProviderRegistry;

use crate::tmdb::TmdbClient;

/// How often to scan all follows. Balanced against TMDB rate limits and
/// indexer politeness — most series ship a new episode once a week, so
/// 4h means we'll surface a new episode within hours of release.
const TICK_INTERVAL: Duration = Duration::from_secs(4 * 60 * 60);

/// Skip per-follow work if its `last_checked_at` is younger than this.
/// Acts as a per-follow rate limit independent of the tick interval —
/// avoids re-hitting a freshly-checked follow if the tick is shifted
/// (e.g., a manual call from the future "force refresh" button).
const PER_FOLLOW_COOLDOWN: Duration = Duration::from_secs(2 * 60 * 60);

/// Today as a `YYYY-MM-DD` string for lex-comparison against TMDB's
/// `air_date`. We only consider an episode grabbable when its air date
/// is on or before today; future-dated episodes stay "à venir".
fn today_iso() -> String {
    Utc::now().format("%Y-%m-%d").to_string()
}

pub fn spawn(
    pool: SqlitePool,
    tmdb: Option<TmdbClient>,
    providers: ProviderRegistry,
) {
    let Some(tmdb) = tmdb else {
        tracing::info!(
            "notify scheduler disabled — TMDB client not configured (no [tmdb] in config)"
        );
        return;
    };
    let providers = Arc::new(providers);
    tokio::spawn(async move {
        // Initial pass after a short warm-up so existing follows get
        // their `available_episodes` populated within seconds of boot,
        // not at the 4 h mark. Without this, every existing follow
        // shows zero `dispo` episodes until the first scheduled tick.
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        run_pass(&pool, &tmdb, providers.clone()).await;

        let mut ticker = tokio::time::interval(TICK_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        ticker.tick().await; // skip the immediate firing
        loop {
            ticker.tick().await;
            run_pass(&pool, &tmdb, providers.clone()).await;
        }
    });
    tracing::info!(
        interval_secs = TICK_INTERVAL.as_secs(),
        "follows notify scheduler started"
    );
}

async fn run_pass(pool: &SqlitePool, tmdb: &TmdbClient, providers: Arc<ProviderRegistry>) {
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
        if let Err(e) = check_one(pool, tmdb, &providers, &follow).await {
            tracing::warn!(
                tmdb_id = follow.tmdb_id,
                name = %follow.name,
                error = %e,
                "scheduler: follow check failed",
            );
        }
        // Bump last_checked_at regardless of success — we don't want a
        // permanently-broken TMDB id to be re-checked on every tick.
        let _ = iris_db::follows::mark_checked(
            pool,
            iris_core::ids::UserId::from(follow.user_id),
            follow.tmdb_id,
        )
        .await;
    }
}

/// Pull follows whose `last_checked_at` is older than the cooldown (or is
/// NULL). Iterates every user's follows — this is a system task, not a
/// per-user one.
async fn all_follows_due(
    pool: &SqlitePool,
) -> Result<Vec<iris_db::follows::FollowRow>, sqlx::Error> {
    use sqlx::Row;
    // Hand-rolled query — `iris_db::follows::list_for_user` is per-user
    // and we want every user's pending follows. Could promote this to a
    // module-level helper if we grow more system-side queries.
    let cutoff = Utc::now() - chrono::Duration::from_std(PER_FOLLOW_COOLDOWN).unwrap();
    sqlx::query(
        "SELECT id, user_id, tmdb_id, name, total_seasons, last_checked_at, \
                last_visited_at, created_at \
         FROM series_follows \
         WHERE last_checked_at IS NULL OR last_checked_at < ?1 \
         ORDER BY last_checked_at ASC NULLS FIRST",
    )
    .bind(cutoff)
    .fetch_all(pool)
    .await
    .map(|rows| {
        rows.into_iter()
            .map(|r| iris_db::follows::FollowRow {
                id: r.get("id"),
                user_id: r.get("user_id"),
                tmdb_id: r.get("tmdb_id"),
                name: r.get("name"),
                total_seasons: r.get("total_seasons"),
                last_checked_at: r.get("last_checked_at"),
                last_visited_at: r.get("last_visited_at"),
                created_at: r.get("created_at"),
            })
            .collect()
    })
}

/// Public entry-point — scan one follow's seasons for available
/// episodes. Used by the scheduler tick AND by the create-follow route
/// (so a freshly followed series's "Sorties" / featured items appear
/// as `dispo` on the Series page immediately, instead of waiting up to
/// 4 hours for the next scheduler tick).
pub async fn scan_follow(
    pool: &SqlitePool,
    tmdb: &TmdbClient,
    providers: &ProviderRegistry,
    follow: &iris_db::follows::FollowRow,
) -> anyhow::Result<()> {
    check_one(pool, tmdb, providers, follow).await
}

async fn check_one(
    pool: &SqlitePool,
    tmdb: &TmdbClient,
    providers: &ProviderRegistry,
    follow: &iris_db::follows::FollowRow,
) -> anyhow::Result<()> {
    let total = follow.total_seasons.unwrap_or(1).max(1);
    let today = today_iso();
    let already = iris_db::episode_files::list_for_tmdb(pool, follow.tmdb_id).await?;
    let already_set: std::collections::HashSet<(i64, i64)> = already
        .iter()
        .map(|r| (r.season, r.episode))
        .collect();

    for season in 1..=total {
        let episodes = tmdb
            .tv_season_episodes(follow.tmdb_id as u64, season as u32)
            .await;
        for ep in episodes {
            // Only consider aired episodes. Future air dates stay "à
            // venir" — surfacing them as available would be a lie.
            if let Some(d) = &ep.air_date {
                if d.as_str() > today.as_str() {
                    continue;
                }
            } else {
                // No air date = TMDB hasn't dated it yet (placeholder
                // entries). Skip until they fill it in.
                continue;
            }
            let key = (season, i64::from(ep.episode));
            if already_set.contains(&key) {
                continue;
            }
            // Skip if we've already cached an availability — no point
            // re-querying every 4h for a slow-aging episode.
            if availability_exists(pool, follow.tmdb_id, season, i64::from(ep.episode)).await? {
                continue;
            }
            try_find_and_record(
                pool,
                providers,
                follow.tmdb_id,
                &follow.name,
                season,
                i64::from(ep.episode),
            )
            .await;
        }
    }
    Ok(())
}

async fn availability_exists(
    pool: &SqlitePool,
    tmdb_id: i64,
    season: i64,
    episode: i64,
) -> Result<bool, sqlx::Error> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM available_episodes \
         WHERE tmdb_id = ?1 AND season = ?2 AND episode = ?3",
    )
    .bind(tmdb_id)
    .bind(season)
    .bind(episode)
    .fetch_one(pool)
    .await?;
    Ok(row.0 > 0)
}

async fn try_find_and_record(
    pool: &SqlitePool,
    providers: &ProviderRegistry,
    tmdb_id: i64,
    series_name: &str,
    season: i64,
    episode: i64,
) {
    let q = SearchQuery {
        q: format!("{series_name} S{season:02}E{episode:02}"),
        page: Some(1),
        limit: Some(20),
        sort_by: Some(SortField::Seeders),
        order: Some(SortOrder::Desc),
        kind: None,
    };
    let agg = providers.search_all(&q).await;
    let Some(best) = pick_best(agg.results) else {
        tracing::debug!(
            tmdb_id, season, episode, series_name,
            "scheduler: no indexer hit",
        );
        return;
    };

    // We deliberately don't call `provider.resolve()` here. Providers
    // that hand back `.torrent` files (torr9 does) would either force
    // us to store opaque bytes in the cache OR get silently skipped —
    // the latter is what was happening before this fix and meant zero
    // torr9 results ever made it into `available_episodes`. Caching
    // the indexer ref is enough to know "this episode is findable";
    // the actual bytes/magnet get fetched via `provider.resolve()` at
    // grab time, when the user is already waiting on a redirect.
    if !providers.ids().iter().any(|id| id == &best.provider_id) {
        // Provider gone between search and now — config reload mid-pass.
        return;
    }
    let upsert = iris_db::available_episodes::UpsertAvailableEpisode {
        tmdb_id,
        season,
        episode,
        indexer_provider: best.provider_id.clone(),
        indexer_torrent_id: best.external_id.clone(),
        // Empty magnet = "re-resolve at grab time". The schema requires
        // a non-null TEXT but the grab endpoint short-circuits when it
        // sees this sentinel and re-queries the provider for the
        // current bytes/magnet.
        magnet: String::new(),
        // Quality hint: pull "1080p" / "720p" out of tags or title best-
        // effort — the frontend doesn't strictly need it but it makes
        // the "Préparer" button copy nicer.
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
            tmdb_id, season, episode,
            provider = %best.provider_id,
            seeders = best.seeders.unwrap_or(0),
            "scheduler: cached new episode",
        );
    }
}

fn pick_best(results: Vec<SearchResult>) -> Option<SearchResult> {
    let mut sorted = results;
    sorted.sort_by_key(|r| std::cmp::Reverse(r.seeders.unwrap_or(0)));
    sorted.into_iter().next()
}

fn extract_quality_from_title(title: &str) -> Option<String> {
    for q in ["2160p", "1080p", "720p", "480p"] {
        if title.contains(q) {
            return Some(q.to_string());
        }
    }
    None
}
