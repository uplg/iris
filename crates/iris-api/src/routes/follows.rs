// File-index / season / episode casts move between i64 (DB) and
// u32/u64 (engine / SCENE parser). Values are domain-bounded, so
// pedantic cast warnings are noise here.
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
)]

//! Per-user series-following endpoints. Mounted under `/api/me/follows`.
//!
//! Identity is the SCENE-normalised name. The Watchlist shelf and
//! Series page run entirely off this — TMDB is consulted only to
//! resolve a poster URL when the joined collection has been
//! `tmdb_verified` (probe runtime match).
//!
//! Episode listings come from two sources, keyed on the same
//! normalised name:
//!   * `episode_files` (via collection join) — what's on disk
//!   * `available_episodes` — what the indexer cached for "Préparer"

use std::collections::BTreeMap;

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use chrono::{DateTime, Utc};
use iris_media::filename::series_key;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::AuthUser;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/episode-context", get(episode_context))
        .route("/{id}", delete(remove))
        .route("/{id}/episodes", get(episodes))
        .route(
            "/{id}/episodes/{season}/{episode}/grab",
            post(grab_episode),
        )
}

// ---------------------------------------------------------------------------
// POST /api/me/follows
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CreateFollowRequest {
    /// The display name from whatever surface the user clicked
    /// (Discovery / Search / `CollectionPage`). Server normalises it
    /// for identity; the original is kept for indexer queries and
    /// UI display.
    name: String,
    /// Optional TMDB id — stored as decoration. Surfaces a poster
    /// only after the corresponding collection gets `tmdb_verified`.
    tmdb_id: Option<i64>,
}

async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<CreateFollowRequest>,
) -> ApiResult<Json<FollowSummary>> {
    let trimmed = body.name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    let normalized = series_key(trimmed);
    if normalized.is_empty() {
        return Err(ApiError::BadRequest("name does not normalise".into()));
    }

    let row = iris_db::follows::add(
        state.db(),
        user.id,
        &normalized,
        trimmed,
        body.tmdb_id,
    )
    .await?;

    // Kick off an immediate background scan so the series page
    // shows `dispo` chips on first visit instead of waiting on
    // the periodic scheduler tick. Best-effort.
    let pool = state.db().clone();
    let providers = state.providers().clone();
    let row_clone = row.clone();
    tokio::spawn(async move {
        if let Err(e) =
            crate::follows_scheduler::scan_follow(&pool, &providers, &row_clone).await
        {
            tracing::warn!(
                follow_id = %row_clone.id,
                name = %row_clone.name,
                error = %e,
                "follow create: initial scan failed",
            );
        }
    });

    Ok(Json(summarize(&state, &row).await))
}

// ---------------------------------------------------------------------------
// GET /api/me/follows
// ---------------------------------------------------------------------------

async fn list(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<FollowSummary>>> {
    let rows = iris_db::follows::list_for_user(state.db(), user.id).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(summarize(&state, &row).await);
    }
    Ok(Json(out))
}

#[derive(Debug, Serialize)]
struct FollowSummary {
    id: Uuid,
    /// SCENE-normalised name — clients route by this, not `tmdb_id`.
    normalized_name: String,
    name: String,
    /// Decoration TMDB id (may be null). Even when present, only
    /// rendered as a poster after the joined collection is verified.
    tmdb_id: Option<i64>,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    new_count: i64,
    last_visited_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

/// Build the client-facing summary. Poster lookup is gated on the
/// matching collection being `tmdb_verified` — without that signal
/// we refuse to fetch TMDB metadata to avoid surfacing the wrong
/// show's poster.
async fn summarize(state: &AppState, row: &iris_db::follows::FollowRow) -> FollowSummary {
    let trusted_tmdb = trusted_tmdb_id(state.db(), &row.normalized_name).await;
    let (poster_path, backdrop_path) = match (state.tmdb(), trusted_tmdb) {
        (Some(client), Some(tid)) => {
            // tid is a positive i64 from the DB; u64 conversion is safe.
            #[allow(clippy::cast_sign_loss)]
            let meta = client.lookup(tid as u64).await;
            meta.map_or((None, None), |m| (m.poster_path, m.backdrop_path))
        }
        _ => (None, None),
    };
    let new_count = iris_db::available_episodes::count_new_for_series(
        state.db(),
        &row.normalized_name,
        row.last_visited_at,
    )
    .await
    .unwrap_or(0);
    FollowSummary {
        id: row.id,
        normalized_name: row.normalized_name.clone(),
        name: row.name.clone(),
        tmdb_id: row.tmdb_id,
        poster_path,
        backdrop_path,
        new_count,
        last_visited_at: row.last_visited_at,
        created_at: row.created_at,
    }
}

/// Returns a TMDB id we trust enough to use for poster lookup —
/// i.e., one stored on a collection whose `tmdb_id` was written by
/// the post-verify enrichment path (which only fires when the
/// runtime probe matched). Returns None when no verified
/// collection joins to this normalised name.
async fn trusted_tmdb_id(
    pool: &iris_db::SqlitePool,
    normalized_name: &str,
) -> Option<i64> {
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT tmdb_id FROM collections \
         WHERE parsed_title_normalized = ?1 AND kind = 'tv' AND tmdb_id IS NOT NULL \
         ORDER BY created_at LIMIT 1",
    )
    .bind(normalized_name)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    row.map(|(t,)| t)
}

// ---------------------------------------------------------------------------
// DELETE /api/me/follows/:id
// ---------------------------------------------------------------------------

async fn remove(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let removed = iris_db::follows::delete(state.db(), user.id, id).await?;
    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

// ---------------------------------------------------------------------------
// GET /api/me/follows/:id/episodes
// ---------------------------------------------------------------------------
//
// SCENE-only: the canonical episode list is the union of
//   * episode_files (on disk)  — keyed on collection_id, join via
//     collections.parsed_title_normalized = follow.normalized_name
//   * available_episodes (indexer cache) — keyed on normalized_name
// Visiting bumps last_visited_at to clear the "X nouveaux" badge.

#[derive(Debug, Deserialize, Default)]
struct EpisodesQuery {
    /// Optional season filter — when set, only that season's rows
    /// are returned. Otherwise everything we know about ships in
    /// one response (covers the grouped Series page render).
    season: Option<u32>,
}

async fn episodes(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<EpisodesQuery>,
) -> ApiResult<Json<EpisodesResponse>> {
    let follow = iris_db::follows::get_by_id(state.db(), user.id, id)
        .await?
        .ok_or(ApiError::NotFound)?;

    // 1. Files on disk — per-collection join via normalised name.
    let downloaded = iris_db::episode_files::list_for_normalized(
        state.db(),
        &follow.normalized_name,
    )
    .await
    .unwrap_or_default();

    // 2. Indexer-cached availability.
    let available = iris_db::available_episodes::list_best_for_series(
        state.db(),
        &follow.normalized_name,
    )
    .await
    .unwrap_or_default();

    // Merge: anything in `downloaded` wins; otherwise fall back to
    // the indexer hint. The two tables can overlap (we ingested an
    // episode that the indexer also still lists) — downloaded
    // status is the higher-signal answer.
    let mut by_key: BTreeMap<(i64, i64), EpisodeItem> = BTreeMap::new();

    for d in &downloaded {
        if let Some(s) = q.season {
            if d.season != i64::from(s) {
                continue;
            }
        }
        let watched = iris_db::playback::get(state.db(), user.id, &d.infohash, d.file_idx)
            .await
            .unwrap_or(None)
            .is_some_and(|p| p.completed);
        by_key.insert(
            (d.season, d.episode),
            EpisodeItem {
                season: d.season,
                episode: d.episode,
                status: EpisodeStatus::Downloaded,
                watched,
                infohash: Some(d.infohash.clone()),
                file_idx: Some(d.file_idx),
                indexer_provider: None,
                indexer_torrent_id: None,
                quality: None,
                seeders: None,
            },
        );
    }
    for a in &available {
        if let Some(s) = q.season {
            if a.season != i64::from(s) {
                continue;
            }
        }
        by_key.entry((a.season, a.episode)).or_insert(EpisodeItem {
            season: a.season,
            episode: a.episode,
            status: EpisodeStatus::Available,
            watched: false,
            infohash: None,
            file_idx: None,
            indexer_provider: Some(a.indexer_provider.clone()),
            indexer_torrent_id: Some(a.indexer_torrent_id.clone()),
            quality: a.quality.clone(),
            seeders: a.seeders,
        });
    }

    let items: Vec<EpisodeItem> = by_key.into_values().collect();

    // Bump visited timestamp AFTER reading — we don't need the
    // previous value past this point.
    let _ = iris_db::follows::mark_visited(state.db(), user.id, id).await;

    Ok(Json(EpisodesResponse {
        season: q.season,
        items,
    }))
}

#[derive(Debug, Serialize)]
struct EpisodesResponse {
    /// Echoes the request filter — `null` when the caller asked for
    /// the full set.
    season: Option<u32>,
    items: Vec<EpisodeItem>,
}

#[derive(Debug, Serialize)]
struct EpisodeItem {
    season: i64,
    episode: i64,
    status: EpisodeStatus,
    watched: bool,
    infohash: Option<String>,
    file_idx: Option<i64>,
    indexer_provider: Option<String>,
    indexer_torrent_id: Option<String>,
    quality: Option<String>,
    seeders: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum EpisodeStatus {
    Downloaded,
    Available,
}

// ---------------------------------------------------------------------------
// GET /api/me/follows/episode-context?infohash=X&file_idx=N
// ---------------------------------------------------------------------------
//
// "Préparer le suivant ?" plumbing for the player. Returns the
// follow id (if any) plus the `(season, episode + 1)` if we know
// it from `available_episodes` or already have it on disk.

#[derive(Debug, Deserialize)]
struct EpisodeContextParams {
    infohash: String,
    file_idx: i64,
}

#[derive(Debug, Serialize)]
struct EpisodeContext {
    followed: bool,
    current: Option<EpisodePoint>,
    next: Option<EpisodePoint>,
}

#[derive(Debug, Serialize)]
struct EpisodePoint {
    follow_id: Option<Uuid>,
    season: i64,
    episode: i64,
    status: EpisodeStatus,
}

async fn episode_context(
    State(state): State<AppState>,
    user: AuthUser,
    Query(p): Query<EpisodeContextParams>,
) -> ApiResult<Json<EpisodeContext>> {
    let Some(current_row) =
        iris_db::episode_files::find_by_file(state.db(), &p.infohash, p.file_idx).await?
    else {
        return Ok(Json(EpisodeContext {
            followed: false,
            current: None,
            next: None,
        }));
    };
    let collection = iris_db::collections::get(state.db(), current_row.collection_id).await?;
    let Some(collection) = collection else {
        return Ok(Json(EpisodeContext {
            followed: false,
            current: None,
            next: None,
        }));
    };
    let Some(normalized) = collection.parsed_title_normalized.as_deref() else {
        // Standalone collection with no SCENE key → no follow can
        // match → no "next" prompt.
        return Ok(Json(EpisodeContext {
            followed: false,
            current: None,
            next: None,
        }));
    };

    let follow = iris_db::follows::get_by_normalized(state.db(), user.id, normalized).await?;

    let current = EpisodePoint {
        follow_id: follow.as_ref().map(|f| f.id),
        season: current_row.season,
        episode: current_row.episode,
        status: EpisodeStatus::Downloaded,
    };

    let Some(follow) = follow else {
        return Ok(Json(EpisodeContext {
            followed: false,
            current: Some(current),
            next: None,
        }));
    };

    let next_se = (current_row.season, current_row.episode + 1);
    let next_status = lookup_episode_status(state.db(), normalized, next_se).await;
    Ok(Json(EpisodeContext {
        followed: true,
        current: Some(current),
        next: next_status.map(|status| EpisodePoint {
            follow_id: Some(follow.id),
            season: next_se.0,
            episode: next_se.1,
            status,
        }),
    }))
}

/// "Do we know about this (S, E) for `normalized`?" Returns the
/// strongest status we have, or None if the indexer cache and disk
/// both come up empty.
async fn lookup_episode_status(
    pool: &iris_db::SqlitePool,
    normalized_name: &str,
    (season, episode): (i64, i64),
) -> Option<EpisodeStatus> {
    let on_disk = iris_db::episode_files::list_for_normalized(pool, normalized_name)
        .await
        .unwrap_or_default()
        .into_iter()
        .any(|r| r.season == season && r.episode == episode);
    if on_disk {
        return Some(EpisodeStatus::Downloaded);
    }
    let avail = iris_db::available_episodes::list_best_for_series(pool, normalized_name)
        .await
        .unwrap_or_default();
    if avail
        .iter()
        .any(|a| a.season == season && a.episode == episode)
    {
        return Some(EpisodeStatus::Available);
    }
    None
}

// ---------------------------------------------------------------------------
// POST /api/me/follows/:id/episodes/:season/:episode/grab
// ---------------------------------------------------------------------------
//
// Routed by follow id (UUID). Idempotent — if the episode is
// already on disk we short-circuit through the existing
// `episode_files` row.

#[derive(Debug, Serialize)]
struct GrabResponse {
    infohash: String,
    file_idx: i64,
    already_grabbed: bool,
}

async fn grab_episode(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, season, episode)): Path<(Uuid, i64, i64)>,
) -> ApiResult<Json<GrabResponse>> {
    let follow = iris_db::follows::get_by_id(state.db(), user.id, id)
        .await?
        .ok_or(ApiError::NotFound)?;

    if let Some(existing) =
        find_episode_file(state.db(), &follow.normalized_name, season, episode).await?
    {
        return Ok(Json(GrabResponse {
            infohash: existing.infohash,
            file_idx: existing.file_idx,
            already_grabbed: true,
        }));
    }

    let pick = match best_available(state.db(), &follow.normalized_name, season, episode).await? {
        Some(p) => p,
        None => find_via_indexer(&state, &follow, season, episode)
            .await?
            .ok_or(ApiError::NotFound)?,
    };

    let result = if pick.magnet.is_empty() {
        let provider = state
            .providers()
            .get(&pick.indexer_provider)
            .ok_or_else(|| {
                ApiError::Internal(anyhow::anyhow!(
                    "provider `{}` no longer registered",
                    pick.indexer_provider
                ))
            })?;
        let source = provider
            .resolve(&pick.indexer_torrent_id)
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("resolve: {e}")))?;
        match source {
            iris_core::search::TorrentSource::Magnet(m) => state.engine().add_from_magnet(&m).await,
            iris_core::search::TorrentSource::TorrentFile(b) => {
                state.engine().add_from_bytes(b).await
            }
        }
    } else {
        state.engine().add_from_magnet(&pick.magnet).await
    }
    .map_err(|e| ApiError::Internal(anyhow::anyhow!("engine: {e}")))?;

    iris_db::torrents::upsert(
        state.db(),
        iris_db::torrents::NewTorrent {
            infohash: result.snapshot.infohash.clone(),
            name: result
                .snapshot
                .name
                .clone()
                .unwrap_or_else(|| format!("{} S{:02}E{:02}", follow.name, season, episode)),
            total_size_bytes: result.snapshot.total_size_bytes,
            source_provider: Some(pick.indexer_provider.clone()),
            source_external_id: Some(pick.indexer_torrent_id.clone()),
            // Carry the follow's optional tmdb_id through to the
            // torrent so the runtime probe can attempt a verify
            // match. Stays unverified until the probe confirms.
            tmdb_id: follow.tmdb_id,
            added_by: user.id,
        },
    )
    .await?;

    let file_idx = pick_largest_video_file(&result.snapshot.files);
    finalise_grabbed_episode(state, &result, follow.tmdb_id, season, episode, file_idx).await?;

    Ok(Json(GrabResponse {
        infohash: result.snapshot.infohash,
        file_idx,
        already_grabbed: result.already_managed,
    }))
}

fn pick_largest_video_file(files: &[iris_torrent::FileEntry]) -> i64 {
    const VIDEO_EXTS: [&str; 10] = [
        "mkv", "mp4", "webm", "m4v", "avi", "mov", "ts", "mts", "m2ts", "wmv",
    ];
    files
        .iter()
        .filter(|f| {
            std::path::Path::new(&f.path)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| VIDEO_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        })
        .max_by_key(|f| f.size_bytes)
        .map_or(0, |f| f.index as i64)
}

/// Run the post-ingest plumbing: collection assignment (synchronous —
/// the Series page relies on the back-reference), then the
/// `episode_files` upsert keyed on the just-attached collection.
async fn finalise_grabbed_episode(
    state: AppState,
    result: &iris_torrent::IngestResult,
    tmdb_id: Option<i64>,
    season: i64,
    episode: i64,
    file_idx: i64,
) -> ApiResult<()> {
    let files: Vec<(usize, String)> = result
        .snapshot
        .files
        .iter()
        .map(|f| (f.index, f.path.clone()))
        .collect();
    crate::collection_assign::assign_after_ingest(
        state.db(),
        state.tmdb(),
        &result.snapshot.infohash,
        &result.snapshot.name.clone().unwrap_or_default(),
        tmdb_id,
        &files,
    )
    .await;

    if let Some(t) =
        iris_db::torrents::find_by_infohash(state.db(), &result.snapshot.infohash).await?
    {
        if let Some(collection_id) = t.collection_id {
            let _ = iris_db::episode_files::upsert(
                state.db(),
                iris_db::episode_files::UpsertEpisodeFile {
                    collection_id,
                    season,
                    episode,
                    infohash: result.snapshot.infohash.clone(),
                    file_idx,
                    derived_from: iris_db::episode_files::DerivedFrom::TmdbMatch,
                },
            )
            .await;
        }
    }
    Ok(())
}

async fn find_episode_file(
    pool: &iris_db::SqlitePool,
    normalized_name: &str,
    season: i64,
    episode: i64,
) -> Result<Option<iris_db::episode_files::EpisodeFileRow>, sqlx::Error> {
    let rows = iris_db::episode_files::list_for_normalized(pool, normalized_name).await?;
    Ok(rows
        .into_iter()
        .find(|r| r.season == season && r.episode == episode))
}

struct PickedAvailability {
    magnet: String,
    indexer_provider: String,
    indexer_torrent_id: String,
}

async fn best_available(
    pool: &iris_db::SqlitePool,
    normalized_name: &str,
    season: i64,
    episode: i64,
) -> Result<Option<PickedAvailability>, sqlx::Error> {
    let rows = iris_db::available_episodes::list_best_for_series(pool, normalized_name).await?;
    Ok(rows
        .into_iter()
        .find(|r| r.season == season && r.episode == episode)
        .map(|r| PickedAvailability {
            magnet: r.magnet,
            indexer_provider: r.indexer_provider,
            indexer_torrent_id: r.indexer_torrent_id,
        }))
}

async fn find_via_indexer(
    state: &AppState,
    follow: &iris_db::follows::FollowRow,
    season: i64,
    episode: i64,
) -> Result<Option<PickedAvailability>, ApiError> {
    use iris_core::search::{SearchQuery, SortField, SortOrder};
    let query = SearchQuery {
        q: format!("{} S{season:02}E{episode:02}", follow.name),
        page: Some(1),
        limit: Some(20),
        sort_by: Some(SortField::Seeders),
        order: Some(SortOrder::Desc),
        kind: None,
    };
    let agg = state.providers().search_all(&query).await;
    let mut sorted = agg.results;
    sorted.sort_by_key(|r| std::cmp::Reverse(r.seeders.unwrap_or(0)));
    let Some(best) = sorted.into_iter().next() else {
        return Ok(None);
    };
    let _ = iris_db::available_episodes::upsert(
        state.db(),
        iris_db::available_episodes::UpsertAvailableEpisode {
            normalized_name: follow.normalized_name.clone(),
            season,
            episode,
            indexer_provider: best.provider_id.clone(),
            indexer_torrent_id: best.external_id.clone(),
            magnet: String::new(),
            quality: None,
            seeders: best.seeders.map(i64::from),
            size_bytes: best.size_bytes.map(|s| s as i64),
        },
    )
    .await;
    Ok(Some(PickedAvailability {
        magnet: String::new(),
        indexer_provider: best.provider_id,
        indexer_torrent_id: best.external_id,
    }))
}
