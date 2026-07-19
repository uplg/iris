// File-index / season / episode casts move between i64 (DB) and
// u32/u64 (engine / SCENE parser). Values are domain-bounded, so
// pedantic cast warnings are noise here.
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
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

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use chrono::{DateTime, Utc};
use iris_media::filename::{Codec, Language, detect_codec, detect_language, series_key};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
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
        .route("/{id}/episodes/{season}/{episode}/grab", post(grab_episode))
}

// POST /api/me/follows

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct CreateFollowRequest {
    /// The display name from whatever surface the user clicked
    /// (Discovery / Search / `CollectionPage`). Server normalises it
    /// for identity; the original is kept for indexer queries and
    /// UI display.
    name: String,
    /// Optional TMDB id — stored as decoration. Surfaces a poster
    /// only after the corresponding collection gets `tmdb_verified`.
    tmdb_id: Option<i64>,
}

#[utoipa::path(
    post,
    path = "/api/me/follows",
    operation_id = "create_follow",
    request_body = CreateFollowRequest,
    responses(
        (status = 200, description = "Created (or existing) follow summary", body = FollowSummary),
        (status = 400, description = "Empty / non-normalisable name"),
    ),
    tag = "follows",
)]
pub(crate) async fn create(
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

    let row =
        iris_db::follows::add(state.db(), user.id, &normalized, trimmed, body.tmdb_id).await?;

    // Kick off an immediate background scan so the series page
    // shows `dispo` chips on first visit instead of waiting on the
    // periodic scheduler tick. Best-effort.
    //
    // The scheduler now keys on `collections`, not `series_follows`,
    // so we look up the TV collection that shares this SCENE
    // identity. The collection may not exist yet (no episode
    // ingested), in which case the scan no-ops — the scheduler will
    // pick it up the moment ingest creates a row.
    let pool = state.db().clone();
    let providers = state.providers().clone();
    let normalized = row.normalized_name.clone();
    let follow_name = row.name.clone();
    let follow_id = row.id;
    tokio::spawn(async move {
        let collection = match iris_db::collections::find_by_parsed_title(
            &pool,
            &normalized,
            iris_db::collections::Kind::Tv,
        )
        .await
        {
            Ok(Some(c)) => c,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!(
                    %follow_id,
                    %follow_name,
                    error = %e,
                    "follow create: collection lookup failed",
                );
                return;
            }
        };
        if let Err(e) =
            crate::collections_scheduler::scan_collection(&pool, &providers, collection.id).await
        {
            tracing::warn!(
                %follow_id,
                %follow_name,
                collection_id = %collection.id,
                error = %e,
                "follow create: initial scan failed",
            );
        }
    });

    Ok(Json(summarize(&state, &row).await))
}

// GET /api/me/follows
//
// C1 façade for APK 0.3.1: per-user Watchlist. With ~10 viewers from
// different families sharing one library, the Watchlist HAS to be
// per-user — series_follows is the right source. The "Follow"
// concept is no longer surfaced anywhere; rows are written
// automatically by `grab_episode_core` on every grab so the user's
// tracked-shows set just reflects what they actually download. APK
// 0.3.1 still calls this exactly the way it did — the only change
// is that the rows it now sees were created implicitly.
// The new web client calls `/api/me/watchlist` (same data, cleaner
// shape) and skips this façade entirely.

#[utoipa::path(
    get,
    path = "/api/me/follows",
    operation_id = "list_follows",
    responses((status = 200, description = "The caller's followed series", body = [FollowSummary])),
    tag = "follows",
)]
pub(crate) async fn list(
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

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FollowSummary {
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
    // `series_follows` is TV-only — hint the namespace so a numerical
    // id collision with a movie can't serve a stranger's poster.
    let (poster_path, backdrop_path) = match (state.tmdb(), trusted_tmdb) {
        (Some(client), Some(tid)) => {
            // tid is a positive i64 from the DB; u64 conversion is safe.
            #[allow(clippy::cast_sign_loss)]
            let meta = client
                .lookup_with_kind(tid as u64, Some(crate::tmdb::TmdbKind::Tv))
                .await;
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
async fn trusted_tmdb_id(pool: &iris_db::SqlitePool, normalized_name: &str) -> Option<i64> {
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

// DELETE /api/me/follows/:id

#[utoipa::path(
    delete,
    path = "/api/me/follows/{id}",
    operation_id = "remove_follow",
    params(("id" = Uuid, Path)),
    responses(
        (status = 204, description = "Unfollowed"),
        (status = 404, description = "No such follow for this user"),
    ),
    tag = "follows",
)]
pub(crate) async fn remove(
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

// GET /api/me/follows/:id/episodes
//
// SCENE-only: the canonical episode list is the union of
//   * episode_files (on disk)  — keyed on collection_id, join via
//     collections.parsed_title_normalized = follow.normalized_name
//   * available_episodes (indexer cache) — keyed on normalized_name
// Visiting bumps last_visited_at to clear the "X nouveaux" badge.

#[derive(Debug, Deserialize, Default, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct EpisodesQuery {
    /// Optional season filter — when set, only that season's rows
    /// are returned. Otherwise everything we know about ships in
    /// one response (covers the grouped Series page render).
    season: Option<u32>,
}

#[utoipa::path(
    get,
    path = "/api/me/follows/{id}/episodes",
    operation_id = "list_follow_episodes",
    params(("id" = Uuid, Path), EpisodesQuery),
    responses(
        (status = 200, description = "Merged on-disk + indexer-available episodes", body = EpisodesResponse),
        (status = 404, description = "Unknown follow / collection id"),
    ),
    tag = "follows",
)]
pub(crate) async fn episodes(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<EpisodesQuery>,
) -> ApiResult<Json<EpisodesResponse>> {
    // Dual resolution: old APK clients pass `series_follows.id`,
    // new clients pass `collection.id`. Both shapes resolve to the
    // same SCENE-normalised name, which is the actual join key.
    let identity = resolve_followish(&state, user.id, id)
        .await
        .ok_or(ApiError::NotFound)?;

    // 1. Files on disk — per-collection join via normalised name.
    let downloaded =
        iris_db::episode_files::list_for_normalized(state.db(), &identity.normalized_name)
            .await
            .unwrap_or_default();

    // 2. Indexer-cached availability.
    let available =
        iris_db::available_episodes::list_best_for_series(state.db(), &identity.normalized_name)
            .await
            .unwrap_or_default();

    // Merge: anything in `downloaded` wins; otherwise fall back to
    // the indexer hint. The two tables can overlap (we ingested an
    // episode that the indexer also still lists) — downloaded
    // status is the higher-signal answer.
    let mut by_key: BTreeMap<(i64, i64), EpisodeItem> = BTreeMap::new();

    for d in &downloaded {
        if let Some(s) = q.season
            && d.season != i64::from(s)
        {
            continue;
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
        if let Some(s) = q.season
            && a.season != i64::from(s)
        {
            continue;
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
    match identity.source {
        FollowishSource::SeriesFollow => {
            let _ = iris_db::follows::mark_visited(state.db(), user.id, id).await;
        }
        FollowishSource::Collection => {
            let _ = iris_db::collections::touch_visited(state.db(), id).await;
        }
    }

    Ok(Json(EpisodesResponse {
        season: q.season,
        items,
    }))
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct EpisodesResponse {
    /// Echoes the request filter — `null` when the caller asked for
    /// the full set.
    season: Option<u32>,
    items: Vec<EpisodeItem>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct EpisodeItem {
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

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum EpisodeStatus {
    Downloaded,
    Available,
}

// GET /api/me/follows/episode-context?infohash=X&file_idx=N
//
// "Préparer le suivant ?" plumbing for the player. Returns the
// follow id (if any) plus the `(season, episode + 1)` if we know
// it from `available_episodes` or already have it on disk.

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct EpisodeContextParams {
    infohash: String,
    file_idx: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct EpisodeContext {
    followed: bool,
    current: Option<EpisodePoint>,
    next: Option<EpisodePoint>,
    /// Previous episode (symmetric to [`Self::next`]). Powers the
    /// "‹ Prev" chip in the TV player so the user can step back
    /// without bouncing through the Series detail screen.
    #[serde(skip_serializing_if = "Option::is_none")]
    prev: Option<EpisodePoint>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct EpisodePoint {
    follow_id: Option<Uuid>,
    season: i64,
    episode: i64,
    status: EpisodeStatus,
    /// Physical location for `status == Downloaded`. Lets the TV
    /// player navigate directly to the next episode without a
    /// round-trip through `/grab`. `None` for `Available` (the file
    /// doesn't exist on disk yet).
    #[serde(skip_serializing_if = "Option::is_none")]
    infohash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_idx: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/me/follows/episode-context",
    operation_id = "follow_episode_context",
    params(EpisodeContextParams),
    responses((status = 200, description = "Prev / current / next episode chain for the player", body = EpisodeContext)),
    tag = "follows",
)]
pub(crate) async fn episode_context(
    State(state): State<AppState>,
    user: AuthUser,
    Query(p): Query<EpisodeContextParams>,
) -> ApiResult<Json<EpisodeContext>> {
    let Some(current_row) =
        iris_db::episode_files::find_by_file(state.db(), &p.infohash, p.file_idx).await?
    else {
        // Standalone or movie file — no episode taxonomy to chain
        // off. Still surface a same-torrent fallback so a season pack
        // with no SCENE-recognised filenames keeps the "next" button
        // working.
        let next = same_torrent_next(state.db(), &p.infohash, p.file_idx + 1).await?;
        let prev = if p.file_idx > 0 {
            same_torrent_next(state.db(), &p.infohash, p.file_idx - 1).await?
        } else {
            None
        };
        return Ok(Json(EpisodeContext {
            followed: false,
            current: None,
            next,
            prev,
        }));
    };
    let collection = iris_db::collections::get(state.db(), current_row.collection_id).await?;
    let normalized = collection
        .as_ref()
        .and_then(|c| c.parsed_title_normalized.as_deref());

    let follow = if let Some(n) = normalized {
        iris_db::follows::get_by_normalized(state.db(), user.id, n).await?
    } else {
        None
    };

    let current = EpisodePoint {
        follow_id: follow.as_ref().map(|f| f.id),
        season: current_row.season,
        episode: current_row.episode,
        status: EpisodeStatus::Downloaded,
        infohash: Some(current_row.infohash.clone()),
        file_idx: Some(current_row.file_idx),
    };

    // 1. Try the next episode within the same season.
    // 2. If that's nowhere (downloaded nor available), try season N+1
    //    episode 1 — covers a binge across a season finale.
    // 3. Finally, fall back to file_idx+1 in the same torrent so
    //    season packs with no follow / collection-context still
    //    surface a "next" button.
    let next_collection = if let Some(n) = normalized {
        let same_season = (current_row.season, current_row.episode + 1);
        let by_same_season = lookup_next_episode(state.db(), follow.as_ref(), n, same_season).await;
        if by_same_season.is_some() {
            by_same_season
        } else {
            let next_season = (current_row.season + 1, 1);
            lookup_next_episode(state.db(), follow.as_ref(), n, next_season).await
        }
    } else {
        None
    };
    let next = match next_collection {
        Some(ep) => Some(ep),
        None => same_torrent_next(state.db(), &p.infohash, p.file_idx + 1).await?,
    };

    // Symmetric previous-episode lookup. (S, E-1), then the last
    // episode of S-1, then same-torrent file_idx-1.
    let prev_collection = if let Some(n) = normalized {
        if current_row.episode > 1 {
            let same_season = (current_row.season, current_row.episode - 1);
            lookup_next_episode(state.db(), follow.as_ref(), n, same_season).await
        } else if current_row.season > 1 {
            // Find the highest-numbered episode of the previous
            // season so the chip can land the user there.
            let prev_season = current_row.season - 1;
            let last_ep = iris_db::episode_files::list_for_normalized(state.db(), n)
                .await
                .unwrap_or_default()
                .into_iter()
                .filter(|r| r.season == prev_season)
                .map(|r| r.episode)
                .max();
            match last_ep {
                Some(ep) => {
                    lookup_next_episode(state.db(), follow.as_ref(), n, (prev_season, ep)).await
                }
                None => None,
            }
        } else {
            None
        }
    } else {
        None
    };
    let prev = match prev_collection {
        Some(ep) => Some(ep),
        None if p.file_idx > 0 => {
            same_torrent_next(state.db(), &p.infohash, p.file_idx - 1).await?
        }
        None => None,
    };

    Ok(Json(EpisodeContext {
        followed: follow.is_some(),
        current: Some(current),
        next,
        prev,
    }))
}

/// Resolve `(season, episode)` for `normalized_name` to an
/// [`EpisodePoint`], preferring on-disk over indexer-cached. Returns
/// `None` when neither layer knows about it. `available` is only
/// surfaced when the user actually follows the series — without a
/// follow they have no `/grab` endpoint to call anyway.
async fn lookup_next_episode(
    pool: &iris_db::SqlitePool,
    follow: Option<&iris_db::follows::FollowRow>,
    normalized_name: &str,
    (season, episode): (i64, i64),
) -> Option<EpisodePoint> {
    let on_disk = iris_db::episode_files::list_for_normalized(pool, normalized_name)
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|r| r.season == season && r.episode == episode);
    if let Some(row) = on_disk {
        return Some(EpisodePoint {
            follow_id: follow.map(|f| f.id),
            season,
            episode,
            status: EpisodeStatus::Downloaded,
            infohash: Some(row.infohash),
            file_idx: Some(row.file_idx),
        });
    }
    let follow = follow?;
    let avail = iris_db::available_episodes::list_best_for_series(pool, normalized_name)
        .await
        .unwrap_or_default();
    if avail
        .iter()
        .any(|a| a.season == season && a.episode == episode)
    {
        return Some(EpisodePoint {
            follow_id: Some(follow.id),
            season,
            episode,
            status: EpisodeStatus::Available,
            infohash: None,
            file_idx: None,
        });
    }
    None
}

/// Last-resort fallback: if there's a sibling file at `file_idx` on
/// the SAME infohash and it has an `episode_files` row (= SCENE
/// parsing recognised it as an episode), return it as a downloaded
/// next. Used when the collection / follow context comes up empty,
/// e.g. season packs that landed without a follow or movies in a
/// folder torrent.
async fn same_torrent_next(
    pool: &iris_db::SqlitePool,
    infohash: &str,
    file_idx: i64,
) -> Result<Option<EpisodePoint>, ApiError> {
    let Some(row) = iris_db::episode_files::find_by_file(pool, infohash, file_idx).await? else {
        return Ok(None);
    };
    Ok(Some(EpisodePoint {
        follow_id: None,
        season: row.season,
        episode: row.episode,
        status: EpisodeStatus::Downloaded,
        infohash: Some(row.infohash),
        file_idx: Some(row.file_idx),
    }))
}

// POST /api/me/follows/:id/episodes/:season/:episode/grab
//
// Routed by follow id (UUID). Idempotent — if the episode is
// already on disk we short-circuit through the existing
// `episode_files` row.

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct GrabResponse {
    pub infohash: String,
    pub file_idx: i64,
    pub already_grabbed: bool,
}

#[utoipa::path(
    post,
    path = "/api/me/follows/{id}/episodes/{season}/{episode}/grab",
    operation_id = "grab_follow_episode",
    params(("id" = Uuid, Path), ("season" = i64, Path), ("episode" = i64, Path)),
    responses(
        (status = 200, description = "Grabbed (or already-owned) episode", body = GrabResponse),
        (status = 404, description = "Unknown follow / no resolvable release"),
    ),
    tag = "follows",
)]
pub(crate) async fn grab_episode(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, season, episode)): Path<(Uuid, i64, i64)>,
) -> ApiResult<Json<GrabResponse>> {
    let identity = resolve_followish(&state, user.id, id)
        .await
        .ok_or(ApiError::NotFound)?;
    // Auto-continuation ("prepare next episode"): keep the series in
    // its established language instead of defaulting to English. We
    // grab in the dominant owned language, falling back gracefully so
    // the next episode still downloads if that language has no offer.
    let dominant = dominant_owned_language(&state, &identity.normalized_name).await;
    grab_episode_core(
        &state,
        GrabEpisodeRequest {
            user_id: user.id,
            normalized_name: &identity.normalized_name,
            display_title: &identity.display_name,
            tmdb_id: identity.tmdb_id,
            season,
            episode,
            language: continuation_pref(dominant),
        },
    )
    .await
    .map(Json)
}

/// Where a "follow-ish" id came from in the dual-resolver flow.
/// APK 0.3.1 round-trips `series_follows.id`; post-0.4 clients
/// round-trip `collections.id`. The handler logic is identical;
/// only the `last_visited_at` bump goes to a different table.
enum FollowishSource {
    SeriesFollow,
    Collection,
}

struct FollowishIdentity {
    normalized_name: String,
    display_name: String,
    tmdb_id: Option<i64>,
    source: FollowishSource,
}

/// Try to resolve an opaque id as either a legacy `series_follows`
/// row (APK 0.3.1) or a `collections` row (post-0.4 clients). The
/// caller never has to care which it was — both produce the same
/// SCENE identity that downstream logic actually keys on.
async fn resolve_followish(
    state: &AppState,
    user_id: iris_core::ids::UserId,
    id: Uuid,
) -> Option<FollowishIdentity> {
    if let Ok(Some(follow)) = iris_db::follows::get_by_id(state.db(), user_id, id).await {
        return Some(FollowishIdentity {
            normalized_name: follow.normalized_name,
            display_name: follow.name,
            tmdb_id: follow.tmdb_id,
            source: FollowishSource::SeriesFollow,
        });
    }
    if let Ok(Some(c)) = iris_db::collections::get(state.db(), id).await
        && c.kind == "tv"
        && let Some(norm) = c.parsed_title_normalized
    {
        return Some(FollowishIdentity {
            normalized_name: norm,
            display_name: c.display_title,
            tmdb_id: c.tmdb_id,
            source: FollowishSource::Collection,
        });
    }
    None
}

/// Shared "grab a specific episode" body. Wraps the look-on-disk →
/// look-in-available_episodes-cache → fall-back-to-live-indexer
/// flow + the ingest plumbing. Both the legacy follows endpoint and
/// the new collection-keyed endpoint (`POST /api/library/collections/
/// :id/grab/:s/:e`) call this — the only difference between callers
/// is how they resolve the SCENE identity (follow row vs collection
/// row).
///
/// `language_pref` gates the live-indexer fallback so an English
/// Seedpool release can't be auto-grabbed for a French collection.
/// Pass `Language::Unknown` to accept any language (first ingest,
/// or genuinely-undecided household).
/// Bundle of every input `grab_episode_core` needs. Lives as a
/// struct (rather than positional args) because the call gathers
/// inputs from a handful of unrelated sources — auth context, SCENE
/// identity resolved off either a follow or a collection, a TMDB
/// id for verify — and "seven args in a row" was a bug magnet at
/// the call site.
pub(crate) struct GrabEpisodeRequest<'a> {
    pub user_id: iris_core::ids::UserId,
    pub normalized_name: &'a str,
    pub display_title: &'a str,
    pub tmdb_id: Option<i64>,
    pub season: i64,
    pub episode: i64,
    /// How the grab path treats language — see [`LangSel`].
    pub language: LangSel,
}

/// Language selection strategy for a grab.
///
/// The three modes encode the three ways a grab is triggered, and they
/// behave differently so each one stays honest:
///
/// * [`LangSel::Any`] — no preference (first ingest of a series, or the
///   legacy APK 0.3.1 follows path with no picker). Take the best offer
///   regardless of language.
/// * [`LangSel::Exact`] — the user clicked a specific language badge in
///   the library. That language or nothing: we never silently hand back
///   a different language. The "Play existing" short-circuit only fires
///   on an owned file in *exactly* that language, and the offer/indexer
///   pick filters strictly. (Fixes: clicking FR opening the owned EN.)
/// * [`LangSel::Prefer`] — auto-continuation ("prepare next episode").
///   Try the languages in order (the series' established language
///   first), then fall back to anything available so the next episode
///   still downloads instead of dead-ending or grabbing EN blindly.
#[derive(Debug, Clone)]
pub(crate) enum LangSel {
    Any,
    Exact(Language),
    Prefer(Vec<Language>),
}

impl LangSel {
    /// Build from an explicit UI language badge: `Some("french")` ⇒
    /// strict `Exact(French)`, `None` ⇒ `Any`. Used by the library grab
    /// button so the chosen language is always honoured.
    pub(crate) fn from_badge(lang: Option<&str>) -> Self {
        match lang {
            Some(l) => LangSel::Exact(Language::parse_tag(l)),
            None => LangSel::Any,
        }
    }
}

/// Pick one item per the selection strategy from `items`, each tagged
/// `(language, sane, value)` and pre-sorted best-first so "first match"
/// is also "best match". `sane` is the shared ranking's
/// alive-and-not-junk flag (always `true` for on-disk files). A sane
/// candidate in a lower-preference language beats a dodgy one in the
/// preferred language — "prepare next episode" must not pick a near-dead
/// FR release over a healthy EN one — but `Exact` never crosses
/// languages, dodgy or not.
fn select_by_lang<T: Clone>(items: &[(Language, bool, T)], sel: &LangSel) -> Option<T> {
    match sel {
        LangSel::Any => items
            .iter()
            .find(|(_, sane, _)| *sane)
            .or_else(|| items.first())
            .map(|(_, _, t)| t.clone()),
        LangSel::Exact(l) => items
            .iter()
            .find(|(lang, sane, _)| lang == l && *sane)
            .or_else(|| items.iter().find(|(lang, _, _)| lang == l))
            .map(|(_, _, t)| t.clone()),
        LangSel::Prefer(order) => order
            .iter()
            .find_map(|w| items.iter().find(|(lang, sane, _)| *sane && lang == w))
            .or_else(|| items.iter().find(|(_, sane, _)| *sane))
            .or_else(|| {
                order
                    .iter()
                    .find_map(|w| items.iter().find(|(lang, _, _)| lang == w))
            })
            .or_else(|| items.first())
            .map(|(_, _, t)| t.clone()),
    }
}

/// Coarse language of an `available_episodes` offer row.
fn offer_language(r: &iris_db::available_episodes::AvailableEpisodeRow) -> Language {
    Language::parse_tag(r.language.as_deref().unwrap_or(""))
}

/// Resolve the language of an on-disk torrent: SCENE name first, the
/// source provider's `default_language` as fallback. Mirrors
/// `library::resolve_torrent_language` so the grab path, the collection
/// view and search all agree on a file's language.
async fn resolve_owned_language(state: &AppState, infohash: &str) -> Language {
    let Some(t) = iris_db::torrents::find_by_infohash(state.db(), infohash)
        .await
        .ok()
        .flatten()
    else {
        return Language::Unknown;
    };
    let detected = detect_language(&t.name);
    if detected != Language::Unknown {
        return detected;
    }
    t.source_provider
        .as_deref()
        .and_then(|p| state.providers().default_language(p))
        .map_or(Language::Unknown, Language::parse_tag)
}

/// The series' established language: the most-owned language across its
/// downloaded episodes. Drives "prepare next episode" so a French
/// series keeps going in French instead of defaulting to English.
/// Priority on a tie / no data follows the household rule "majority FR,
/// else EN, else MULTI".
pub(crate) async fn dominant_owned_language(state: &AppState, normalized_name: &str) -> Language {
    fn tally(counts: &mut (u32, u32, u32), lang: Language) {
        match lang {
            Language::French => counts.0 += 1,
            Language::English => counts.1 += 1,
            Language::Multi => counts.2 += 1,
            Language::Unknown => {}
        }
    }
    let files = iris_db::episode_files::list_for_normalized(state.db(), normalized_name)
        .await
        .unwrap_or_default();
    let mut counts = (0u32, 0u32, 0u32);
    for f in &files {
        tally(
            &mut counts,
            resolve_owned_language(state, &f.infohash).await,
        );
    }
    if counts == (0, 0, 0) {
        // Nothing on disk (typically a garbage-collected series) — fall
        // back to what the household actually *watched*. Playback rows
        // survive reclaim (torrents are only soft-deleted), so a series
        // watched in MULTi keeps continuing in MULTi even after its
        // files are gone.
        let watched =
            iris_db::playback::watched_infohashes_for_normalized(state.db(), normalized_name)
                .await
                .unwrap_or_default();
        for ih in &watched {
            tally(&mut counts, resolve_owned_language(state, ih).await);
        }
    }
    // "Majority FR ⇒ FR, else EN, else MULTI": French only on a strict
    // plurality; otherwise English wins (including ties and the
    // nothing-owned default), with Multi as the last resort.
    let (fr, en, multi) = counts;
    if fr > en && fr > multi {
        Language::French
    } else if en >= multi {
        Language::English
    } else {
        Language::Multi
    }
}

/// Build the ordered language preference for an auto-continuation grab:
/// the series' established language first, then MULTI, FR, EN as
/// graceful fallbacks (deduped, first occurrence wins). MULTI leads the
/// fallbacks because it carries both audio tracks — it substitutes for
/// any dominant language; French outranks English per the household
/// rule (a Multi watcher falls back to FR before EN).
pub(crate) fn continuation_pref(dominant: Language) -> LangSel {
    let mut order = vec![dominant];
    for l in [Language::Multi, Language::French, Language::English] {
        if !order.contains(&l) {
            order.push(l);
        }
    }
    LangSel::Prefer(order)
}

/// The series' established release profile — the resolution and codec
/// the household already owns it in. Grabs *prefer* (never require) a
/// matching release, and the preference dominates: a 2160p must never
/// beat a 1080p when the series is established in 1080p, even if the
/// 2160p is better seeded — a slow 1080p download still beats a 4K
/// monster the boxes may not even decode. Only the anti-junk size
/// guard outranks the profile (a 10 MB "1080p" sample never wins);
/// confirmed-dead 0-seeder offers are excluded upstream.
struct GrabProfile {
    /// Preferred resolution tag (`"1080p"`…). `Some("1080p")` by
    /// default when the series has no usable history — the household
    /// standard, and it stops a fresh series from starting on a 4K
    /// monster.
    quality: Option<String>,
    /// Preferred codec (`Codec::as_str` form). `None` when no owned
    /// file carries a codec tag — codec stays free.
    codec: Option<String>,
}

/// First key with the highest count — `BTreeMap` iteration is
/// key-ordered, so ties break deterministically on the smaller tag.
fn majority_tag(counts: BTreeMap<String, u32>) -> Option<String> {
    let best = counts.values().copied().max()?;
    counts.into_iter().find(|(_, c)| *c == best).map(|(k, _)| k)
}

/// Derive the series' [`GrabProfile`] from the distinct owned torrents
/// backing its episode files (a season pack counts once, not once per
/// episode). Mirrors [`dominant_owned_language`]'s "follow what's
/// established" idea for resolution + codec.
async fn dominant_owned_profile(
    state: &AppState,
    owned_files: &[iris_db::episode_files::EpisodeFileRow],
) -> GrabProfile {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut quality_counts: BTreeMap<String, u32> = BTreeMap::new();
    let mut codec_counts: BTreeMap<String, u32> = BTreeMap::new();
    for f in owned_files {
        if !seen.insert(f.infohash.as_str()) {
            continue;
        }
        let Some(t) = iris_db::torrents::find_by_infohash(state.db(), &f.infohash)
            .await
            .ok()
            .flatten()
        else {
            continue;
        };
        if let Some(q) = iris_media::filename::parse(&t.name).and_then(|p| p.quality) {
            *quality_counts.entry(q).or_default() += 1;
        }
        let codec = detect_codec(&t.name);
        if codec != Codec::Unknown {
            *codec_counts.entry(codec.as_str().to_string()).or_default() += 1;
        }
    }
    GrabProfile {
        quality: majority_tag(quality_counts).or_else(|| Some("1080p".to_string())),
        codec: majority_tag(codec_counts),
    }
}

/// Preference rank of an offer's tag against the profile's: 0 = match,
/// 1 = untagged (tolerated — absence of evidence), 2 = known mismatch.
/// No preference → everything ranks 0.
fn tag_rank(offer: Option<&str>, preferred: Option<&str>) -> u8 {
    let Some(p) = preferred else { return 0 };
    match offer {
        Some(o) if o.eq_ignore_ascii_case(p) => 0,
        None => 1,
        Some(_) => 2,
    }
}

/// An offer's codec tag with the `"unknown"` sentinel collapsed to
/// `None` so [`tag_rank`] treats both spellings as untagged.
fn offer_codec(r: &iris_db::available_episodes::AvailableEpisodeRow) -> Option<&str> {
    r.codec.as_deref().filter(|c| *c != "unknown")
}

/// View a cached offer through the shared ranking lens.
fn offer_candidate(
    r: &iris_db::available_episodes::AvailableEpisodeRow,
) -> iris_core::ranking::Candidate {
    iris_core::ranking::Candidate {
        seeders: r.seeders,
        size_bytes: r.size_bytes,
        is_multi: offer_language(r) == Language::Multi,
    }
}

/// Profile-aware grab ordering over cached offers: anti-junk guard
/// first (a sample-sized "1080p" never wins), then format match, then
/// codec match, then aliveness (the [`iris_core::ranking::SEED_FLOOR`]
/// garde-fou — demoted *below* the profile so a low-seeded 1080p still
/// beats a healthy 2160p when the series is established in 1080p),
/// then the shared recommended policy (smallest size, seeders as
/// tie-break).
fn offer_cmp(
    a: &iris_db::available_episodes::AvailableEpisodeRow,
    b: &iris_db::available_episodes::AvailableEpisodeRow,
    profile: &GrabProfile,
) -> Ordering {
    let (ca, cb) = (offer_candidate(a), offer_candidate(b));
    let q = profile.quality.as_deref();
    let c = profile.codec.as_deref();
    cb.big_enough()
        .cmp(&ca.big_enough())
        .then_with(|| tag_rank(a.quality.as_deref(), q).cmp(&tag_rank(b.quality.as_deref(), q)))
        .then_with(|| tag_rank(offer_codec(a), c).cmp(&tag_rank(offer_codec(b), c)))
        .then_with(|| cb.alive().cmp(&ca.alive()))
        .then_with(|| iris_core::ranking::recommended_cmp(&ca, &cb))
}

/// What the grab resolution settled on: a season pack to ingest whole,
/// or a singleton release for the requested episode.
enum GrabSource {
    Pack(PickedAvailability),
    Singleton(PickedAvailability),
}

/// Decide where the requested episode comes from. A live indexer sweep
/// for the (S, E) runs FIRST: cached seeder counts are scan-time
/// snapshots and a release can die between scheduler passes, while the
/// rule is absolute — a confirmed 0-seeder release is never grabbed.
/// Fresh counts are written back onto the cached rows (a confirmed 0
/// poisons the row for every surface — grabs, badges, packs), then
/// resolution order (every path honours `language` and the series'
/// established format/codec profile):
///   1. Season-boundary pack-first: entering a season the household
///      owns nothing of (just finished S3 → wants S4) takes the full
///      season pack when a *sane*, liveness-verified one is cached —
///      the user wants the season, not ten sequential singleton grabs.
///   2. Live singleton for the requested (S, E) — fresh seeders decide.
///   3. Cached singleton — fallback when the sweep came back empty
///      (indexer down / release not surfaced by tvsearch); rows the
///      sweep confirmed dead are already excluded by the write-back.
///   4. Cached season pack covering the requested season, liveness-
///      verified — last resort when no singleton offer exists.
async fn resolve_grab_source(
    state: &AppState,
    normalized_name: &str,
    display_title: &str,
    season: i64,
    episode: i64,
    language: &LangSel,
) -> ApiResult<GrabSource> {
    // Established profile: follow the resolution / codec the household
    // already owns this series in.
    let owned_files = iris_db::episode_files::list_for_normalized(state.db(), normalized_name)
        .await
        .unwrap_or_default();
    let profile = dominant_owned_profile(state, &owned_files).await;

    let live = live_results(state, display_title, season, Some(episode)).await;
    let cached = iris_db::available_episodes::list_offers_for_episode(
        state.db(),
        normalized_name,
        season,
        episode,
    )
    .await?;
    refresh_offer_liveness(state.db(), &cached, &live).await;

    let owns_any_in_season = owned_files.iter().any(|f| f.season == season);
    if !owns_any_in_season
        && let Some((pack, true)) = verified_pack_offer(
            state,
            normalized_name,
            display_title,
            season,
            language,
            &profile,
        )
        .await?
    {
        return Ok(GrabSource::Pack(pack));
    }
    if let Some(pick) = pick_live_singleton(
        state,
        normalized_name,
        language,
        &profile,
        season,
        episode,
        live,
    )
    .await
    {
        return Ok(GrabSource::Singleton(pick));
    }
    if let Some(pick) = best_available(
        state.db(),
        normalized_name,
        season,
        episode,
        language,
        &profile,
    )
    .await?
    {
        return Ok(GrabSource::Singleton(pick));
    }
    if let Some((pack, _)) = verified_pack_offer(
        state,
        normalized_name,
        display_title,
        season,
        language,
        &profile,
    )
    .await?
    {
        return Ok(GrabSource::Pack(pack));
    }
    Err(ApiError::NotFound)
}

pub(crate) async fn grab_episode_core(
    state: &AppState,
    req: GrabEpisodeRequest<'_>,
) -> ApiResult<GrabResponse> {
    let GrabEpisodeRequest {
        user_id,
        normalized_name,
        display_title,
        tmdb_id,
        season,
        episode,
        language,
    } = req;

    // Auto-track for this user. With a multi-family household
    // (~10 viewers, mixed taste) the Watchlist is per-user; the act
    // of grabbing an episode is the strongest "I want to keep
    // watching this" signal we have. Fire before the on-disk short-
    // circuit so a "grab the one I already have" click still tracks.
    // Idempotent — `iris_db::follows::add` is a no-op when
    // (user_id, normalized_name) already exists.
    let _ =
        iris_db::follows::add(state.db(), user_id, normalized_name, display_title, tmdb_id).await;

    // Short-circuit only when we already hold the episode in the
    // requested language. An explicit FR badge click must NOT return
    // the owned EN file — `find_episode_file` honours `language` so the
    // grab proceeds and fetches the FR release instead.
    if let Some(existing) =
        find_episode_file(state, normalized_name, season, episode, &language).await?
    {
        return Ok(GrabResponse {
            infohash: existing.infohash,
            file_idx: existing.file_idx,
            already_grabbed: true,
        });
    }

    let pick = match resolve_grab_source(
        state,
        normalized_name,
        display_title,
        season,
        episode,
        &language,
    )
    .await?
    {
        GrabSource::Pack(pack) => {
            return ingest_pack_and_pick_episode(
                state,
                user_id,
                display_title,
                pack,
                season,
                episode,
            )
            .await;
        }
        GrabSource::Singleton(pick) => pick,
    };

    let result = ingest_picked(
        state,
        &pick,
        ReprimeHint {
            display_title,
            season,
            episode: Some(episode),
        },
    )
    .await?;

    iris_db::torrents::upsert(
        state.db(),
        iris_db::torrents::NewTorrent {
            infohash: result.snapshot.infohash.clone(),
            name: result
                .snapshot
                .name
                .clone()
                .unwrap_or_else(|| format!("{display_title} S{season:02}E{episode:02}")),
            total_size_bytes: result.snapshot.total_size_bytes,
            source_provider: Some(pick.indexer_provider.clone()),
            source_external_id: Some(pick.indexer_torrent_id.clone()),
            added_by: user_id,
        },
    )
    .await?;

    let file_idx = pick_largest_video_file(&result.snapshot.files);
    finalise_grabbed_episode(state.clone(), &result, season, episode, file_idx).await?;

    Ok(GrabResponse {
        infohash: result.snapshot.infohash,
        file_idx,
        already_grabbed: result.already_managed,
    })
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
        crate::collection_assign::EnrichDeps {
            tmdb: state.tmdb(),
            anilist: state.anilist(),
            providers: Some(state.providers()),
        },
        &result.snapshot.infohash,
        &result.snapshot.name.clone().unwrap_or_default(),
        &files,
    )
    .await;

    if let Some(t) =
        iris_db::torrents::find_by_infohash(state.db(), &result.snapshot.infohash).await?
        && let Some(collection_id) = t.collection_id
    {
        // Mirror the parser's SxxExx absolute rule: a fleuve grab
        // arrives as `season=1, episode=<absolute>`, so a high
        // episode under season 1 carries the absolute number.
        let absolute_episode = (season == 1
            && episode > i64::from(iris_media::filename::ABSOLUTE_EPISODE_THRESHOLD))
        .then_some(episode);
        let _ = iris_db::episode_files::upsert(
            state.db(),
            iris_db::episode_files::UpsertEpisodeFile {
                collection_id,
                season,
                episode,
                infohash: result.snapshot.infohash.clone(),
                file_idx,
                derived_from: iris_db::episode_files::DerivedFrom::TmdbMatch,
                absolute_episode,
            },
        )
        .await;
    }
    Ok(())
}

/// Look for the requested episode already on disk, honouring the
/// language selection. `LangSel::Exact(FR)` matches only an owned FR
/// file — so an explicit FR grab never short-circuits to the owned EN
/// (the user's "clicking FR opens English" bug). `Any` keeps the legacy
/// "first file wins" behaviour and skips the per-file language probe.
async fn find_episode_file(
    state: &AppState,
    normalized_name: &str,
    season: i64,
    episode: i64,
    sel: &LangSel,
) -> Result<Option<iris_db::episode_files::EpisodeFileRow>, sqlx::Error> {
    let rows = iris_db::episode_files::list_for_normalized(state.db(), normalized_name).await?;
    let candidates: Vec<_> = rows
        .into_iter()
        .filter(|r| r.season == season && r.episode == episode)
        .collect();
    if matches!(sel, LangSel::Any) {
        return Ok(candidates.into_iter().next());
    }
    let mut tagged = Vec::with_capacity(candidates.len());
    for c in candidates {
        let lang = resolve_owned_language(state, &c.infohash).await;
        // On-disk files are always "sane" — the download already completed.
        tagged.push((lang, true, c));
    }
    Ok(select_by_lang(&tagged, sel))
}

struct PickedAvailability {
    magnet: String,
    indexer_provider: String,
    indexer_torrent_id: String,
    /// Pre-signed `.torrent` URL persisted on `available_episodes`
    /// at scan time. When set, the grab path fetches the bytes
    /// directly and skips `provider.resolve()` entirely — that's
    /// the in-memory-cache fallback path that 500s after restarts
    /// for Torznab / UNIT3D providers.
    download_url: Option<String>,
}

/// Context the grab path has handy that `provider.resolve()` doesn't —
/// pass it down so we can re-prime an empty link cache by doing a
/// targeted search. Restart-survivor mechanism #2 (the persisted
/// `download_url` is #1; this is what kicks in when the URL is null
/// — old DB rows pre-migration 0018 — or when the URL has 404'd).
struct ReprimeHint<'a> {
    /// Display title for the SCENE-form search query.
    display_title: &'a str,
    season: i64,
    /// `None` for season-pack grabs ("Show.Name.S01"); `Some` for
    /// individual-episode grabs.
    episode: Option<i64>,
}

async fn best_available(
    pool: &iris_db::SqlitePool,
    normalized_name: &str,
    season: i64,
    episode: i64,
    sel: &LangSel,
    profile: &GrabProfile,
) -> Result<Option<PickedAvailability>, sqlx::Error> {
    let mut rows = iris_db::available_episodes::list_offers_for_episode(
        pool,
        normalized_name,
        season,
        episode,
    )
    .await?;
    // Rank the full candidate set profile-aware (sane → format match →
    // codec match → smallest size / seeders). Best-first order means the
    // language walk's "first match per language" is also best-in-language.
    // `Exact` never substitutes another language (a clicked FR badge that
    // has no FR offer 404s rather than grabbing EN); `Prefer` walks the
    // continuation order sane-first, then accepts anything available.
    rows.sort_by(|a, b| offer_cmp(a, b, profile));
    let tagged: Vec<(
        Language,
        bool,
        iris_db::available_episodes::AvailableEpisodeRow,
    )> = rows
        .into_iter()
        .map(|r| (offer_language(&r), offer_candidate(&r).sane(), r))
        .collect();
    Ok(select_by_lang(&tagged, sel).map(|r| PickedAvailability {
        magnet: r.magnet,
        indexer_provider: r.indexer_provider,
        indexer_torrent_id: r.indexer_torrent_id,
        download_url: r.download_url,
    }))
}

/// Resolve a `PickedAvailability` (magnet or provider-hosted
/// `.torrent`) through librqbit and return the runtime
/// [`IngestResult`]. Shared between the singleton grab and the
/// season-pack grab — both paths go through identical engine
/// plumbing, only the post-ingest "which file is the user's pick"
/// resolution differs.
async fn ingest_picked(
    state: &AppState,
    pick: &PickedAvailability,
    reprime: ReprimeHint<'_>,
) -> ApiResult<iris_torrent::IngestResult> {
    // Resolution order:
    //   1. Magnet — pre-resolved magnet URI, hand straight to librqbit.
    //   2. Persisted download_url — scheduler stashed a pre-signed
    //      `.torrent` URL at scan time. Fetch via the provider's
    //      `fetch_bytes` and feed bytes to the engine. Bypasses
    //      `provider.resolve()`'s in-memory link cache, which
    //      evaporates on restart and caused real 500s when c411 /
    //      UNIT3D dropped older releases off the search top page.
    //   3. provider.resolve() — last resort: per-id round-trip to
    //      the indexer. Works for torr9-style providers that don't
    //      ship a URL in the search payload, and as a fallback when
    //      the persisted URL has expired.
    if !pick.magnet.is_empty() {
        return state
            .engine()
            .add_from_magnet(&pick.magnet)
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("engine: {e}")));
    }
    if let Some(url) = pick.download_url.as_deref()
        && let Some(provider) = state.providers().get(&pick.indexer_provider)
    {
        match provider.fetch_bytes(url).await {
            Ok(bytes) => {
                return state
                    .engine()
                    .add_from_bytes(bytes.to_vec())
                    .await
                    .map_err(|e| ApiError::Internal(anyhow::anyhow!("engine: {e}")));
            }
            Err(e) => {
                tracing::warn!(
                    url,
                    provider = %pick.indexer_provider,
                    error = %e,
                    "persisted download_url fetch failed; falling back to provider.resolve()",
                );
            }
        }
    }
    let provider = state
        .providers()
        .get(&pick.indexer_provider)
        .ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "provider `{}` no longer registered",
                pick.indexer_provider
            ))
        })?;
    // First resolve attempt — works when the provider's in-memory
    // link cache is still hot (i.e. the scheduler has touched this
    // (S, E) recently). On a fresh server boot the cache is empty
    // and Torznab / UNIT3D providers refuse the lookup.
    let source = match provider.resolve(&pick.indexer_torrent_id).await {
        Ok(s) => s,
        Err(first_err) => {
            // Re-prime: do a targeted search that, by side effect,
            // refills the provider's link cache for this exact
            // release. We have the show title + S/E because the
            // grab path knows them — `resolve()` doesn't get them
            // through the trait. After the re-prime, the second
            // `resolve()` lookup hits the cache.
            tracing::info!(
                provider = %pick.indexer_provider,
                external_id = %pick.indexer_torrent_id,
                reason = %first_err,
                "resolve cache miss — repriming via search and retrying",
            );
            let prime_q = build_reprime_query(&reprime);
            let _ = provider.search(&prime_q).await;
            provider
                .resolve(&pick.indexer_torrent_id)
                .await
                .map_err(|e| {
                    ApiError::Internal(anyhow::anyhow!(
                        "resolve after re-prime: {e} (first attempt: {first_err})"
                    ))
                })?
        }
    };
    match source {
        iris_core::search::TorrentSource::Magnet(m) => state.engine().add_from_magnet(&m).await,
        iris_core::search::TorrentSource::TorrentFile(b) => state.engine().add_from_bytes(b).await,
    }
    .map_err(|e| ApiError::Internal(anyhow::anyhow!("engine: {e}")))
}

/// Build the targeted `SearchQuery` used to re-prime a provider's
/// link cache after a `resolve()` miss. The episode field decides
/// pack vs singleton: `Some` produces `Show.Name S04E11`, `None`
/// produces `Show.Name S04` so a season-pack grab finds its
/// pack-shaped offer.
fn build_reprime_query(reprime: &ReprimeHint<'_>) -> iris_core::search::SearchQuery {
    use iris_core::search::{SearchQuery, SortField, SortOrder};
    let q = match reprime.episode {
        Some(e) => format!(
            "{title} S{s:02}E{e:02}",
            title = reprime.display_title,
            s = reprime.season,
            e = e,
        ),
        None => format!(
            "{title} S{s:02}",
            title = reprime.display_title,
            s = reprime.season,
        ),
    };
    SearchQuery {
        q,
        page: Some(1),
        limit: Some(20),
        sort_by: Some(SortField::Seeders),
        order: Some(SortOrder::Desc),
        kind: None,
        parsed_title: Some(iris_media::filename::series_key(reprime.display_title)),
        season: Some(reprime.season as u32),
        episode: reprime.episode.map(|e| e as u32),
        year: None,
    }
}

/// Look up a cached season pack that can satisfy a (season, episode)
/// request — the season-boundary pack-first rule and the no-singleton
/// fallback both come through here. Applies the same language selection
/// as the singleton path (`Exact` won't drop an "FR" click into an
/// English pack; `Prefer` walks the continuation order) and the same
/// format/codec profile preference. Returns the pick together with its
/// sanity flag so the pack-first caller can insist on a *sane* pack
/// while the last-resort fallback accepts anything still alive.
///
/// No redundancy filtering here: unlike the display list, a pack that
/// `list_season_packs_for_series`'s display-side caller would hide as
/// "redundant" (e.g. an FR pack when Multi is owned) must still be
/// grabbable if that's genuinely the only way to cover this leaf.
async fn find_pack_offer(
    pool: &iris_db::SqlitePool,
    normalized_name: &str,
    season: i64,
    sel: &LangSel,
    profile: &GrabProfile,
) -> Result<Option<(PickedAvailability, bool)>, sqlx::Error> {
    let mut packs =
        iris_db::available_episodes::list_pack_offers_for_season(pool, normalized_name, season)
            .await?;
    packs.sort_by(|a, b| offer_cmp(a, b, profile));
    let tagged: Vec<(
        Language,
        bool,
        iris_db::available_episodes::AvailableEpisodeRow,
    )> = packs
        .into_iter()
        .map(|p| (offer_language(&p), offer_candidate(&p).sane(), p))
        .collect();
    Ok(select_by_lang(&tagged, sel).map(|p| {
        let sane = offer_candidate(&p).sane();
        (
            PickedAvailability {
                magnet: p.magnet,
                indexer_provider: p.indexer_provider,
                indexer_torrent_id: p.indexer_torrent_id,
                download_url: p.download_url,
            },
            sane,
        )
    }))
}

/// Ingest a season pack and resolve a specific (season, episode)
/// leaf inside it. Runs the same engine ingest path as the singleton
/// grab; the difference is post-ingest: instead of returning the
/// pack's "main video", we SCENE-parse every file and pick the one
/// whose `(season, episode)` matches the request. If the parser
/// can't find the requested episode inside the pack we surface a
/// `NotFound` — the user clicked a non-existent (S, E).
async fn ingest_pack_and_pick_episode(
    state: &AppState,
    user_id: iris_core::ids::UserId,
    display_title: &str,
    pack: PickedAvailability,
    season: i64,
    episode: i64,
) -> ApiResult<GrabResponse> {
    let result = ingest_picked(
        state,
        &pack,
        // Re-prime hint targets the season pack, not the individual
        // episode — a c411 search of "Show.Name S04" turns up the
        // pack entry, refreshing the link cache the resolver needs.
        ReprimeHint {
            display_title,
            season,
            episode: None,
        },
    )
    .await?;

    // Find the leaf matching the requested (S, E). SCENE-parse each
    // path; pick the one whose `(season, episode)` matches. The
    // engine's snapshot already orders files SCENE-aware, but we
    // can't trust position alone — a multi-disc pack might have
    // `Disc1/Show.S01E04.mkv` ahead of `Disc2/Show.S01E12.mkv`
    // alphabetically without that matching the requested episode.
    let file_idx = result
        .snapshot
        .files
        .iter()
        .find_map(|f| {
            let leaf = f.path.rsplit('/').next().unwrap_or(&f.path);
            let parsed = iris_media::filename::parse(leaf)?;
            let s = parsed.season?;
            let e = parsed.episode?;
            if i64::from(s) == season && i64::from(e) == episode {
                Some(f.index as i64)
            } else {
                None
            }
        })
        .ok_or_else(|| ApiError::NotFound)?;

    iris_db::torrents::upsert(
        state.db(),
        iris_db::torrents::NewTorrent {
            infohash: result.snapshot.infohash.clone(),
            name: result
                .snapshot
                .name
                .clone()
                .unwrap_or_else(|| format!("{display_title} S{season:02} pack")),
            total_size_bytes: result.snapshot.total_size_bytes,
            source_provider: Some(pack.indexer_provider.clone()),
            source_external_id: Some(pack.indexer_torrent_id.clone()),
            added_by: user_id,
        },
    )
    .await?;

    // Same finalisation as the singleton path — collection_assign
    // will SCENE-parse every file in the pack and create
    // episode_files for the FULL season in one shot, so subsequent
    // calls for sibling episodes short-circuit through the on-disk
    // check.
    finalise_grabbed_episode(state.clone(), &result, season, episode, file_idx).await?;

    Ok(GrabResponse {
        infohash: result.snapshot.infohash,
        file_idx,
        already_grabbed: result.already_managed,
    })
}

/// View a live search result through the shared ranking lens.
fn result_candidate(r: &iris_core::search::SearchResult) -> iris_core::ranking::Candidate {
    iris_core::ranking::Candidate {
        seeders: r.seeders.map(i64::from),
        size_bytes: r.size_bytes.and_then(|s| i64::try_from(s).ok()),
        is_multi: detect_language(&r.title) == Language::Multi,
    }
}

/// One live indexer sweep for a `(season, episode)` — or the season's
/// pack shape when `episode` is `None`. The grab path runs this before
/// trusting the offer cache: cached seeder counts are scan-time
/// snapshots, and fresh counts are the only signal that can honour
/// "never grab a dead torrent". Provider failures degrade to a partial
/// or empty result set, never an error.
async fn live_results(
    state: &AppState,
    display_title: &str,
    season: i64,
    episode: Option<i64>,
) -> Vec<iris_core::search::SearchResult> {
    let query = build_reprime_query(&ReprimeHint {
        display_title,
        season,
        episode,
    });
    state.providers().search_all(&query).await.results
}

/// Fresh seeder count for a cached offer, when the live sweep surfaced
/// the same release (same provider + external id). `None` = not seen by
/// the sweep — absence is not proof of death, the cached count stands.
fn matching_live_seeders(
    row: &iris_db::available_episodes::AvailableEpisodeRow,
    live: &[iris_core::search::SearchResult],
) -> Option<i64> {
    live.iter()
        .find(|r| r.provider_id == row.indexer_provider && r.external_id == row.indexer_torrent_id)
        .and_then(|r| r.seeders.map(i64::from))
}

/// Write live seeder counts back onto cached offer rows. A release the
/// sweep confirms at 0 seeders gets its row poisoned — every reader
/// filters `seeders IS NOT 0`, so the dead offer disappears from grab
/// candidates and availability badges alike, not just from this grab.
async fn refresh_offer_liveness(
    pool: &iris_db::SqlitePool,
    rows: &[iris_db::available_episodes::AvailableEpisodeRow],
    live: &[iris_core::search::SearchResult],
) {
    for row in rows {
        if let Some(s) = matching_live_seeders(row, live)
            && row.seeders != Some(s)
            && let Err(e) = iris_db::available_episodes::set_seeders(pool, row.id, s).await
        {
            tracing::warn!(error = %e, offer = %row.id, "failed to refresh offer seeders");
        }
    }
}

/// [`find_pack_offer`] with a liveness re-check: when the cache holds a
/// candidate pack, run one live season sweep, write the fresh seeder
/// counts back, and re-pick. A pack that died since the scheduler last
/// saw it (offers for garbage-collected series linger in the cache for
/// months) stops being offered the moment the sweep confirms 0 seeders.
async fn verified_pack_offer(
    state: &AppState,
    normalized_name: &str,
    display_title: &str,
    season: i64,
    sel: &LangSel,
    profile: &GrabProfile,
) -> Result<Option<(PickedAvailability, bool)>, sqlx::Error> {
    // Cheap cache pre-check before paying for a provider fan-out.
    if find_pack_offer(state.db(), normalized_name, season, sel, profile)
        .await?
        .is_none()
    {
        return Ok(None);
    }
    let live = live_results(state, display_title, season, None).await;
    let rows = iris_db::available_episodes::list_pack_offers_for_season(
        state.db(),
        normalized_name,
        season,
    )
    .await?;
    refresh_offer_liveness(state.db(), &rows, &live).await;
    find_pack_offer(state.db(), normalized_name, season, sel, profile).await
}

/// Pick the best singleton release from a live sweep's results and
/// record it in the offer cache. Same language selection and
/// profile-aware ordering as the cached paths, so "first match per
/// language" is the best one; `Exact` keeps only the requested language
/// (404 if none — a clicked FR badge mustn't pull EN), `Prefer` walks
/// the continuation order sane-first.
async fn pick_live_singleton(
    state: &AppState,
    normalized_name: &str,
    sel: &LangSel,
    profile: &GrabProfile,
    season: i64,
    episode: i64,
    mut results: Vec<iris_core::search::SearchResult>,
) -> Option<PickedAvailability> {
    // Dead-torrent guard: never offer a confirmed 0-seeder release (its pieces
    // would never fully assemble). Unknown / ≥1 seeders pass through.
    results.retain(|r| r.seeders != Some(0));
    // Decorate before sorting — parsing the title in the comparator
    // would re-run the SCENE parser O(n log n) times.
    let q_pref = profile.quality.as_deref();
    let c_pref = profile.codec.as_deref();
    let mut decorated: Vec<(
        iris_core::ranking::Candidate,
        u8,
        u8,
        iris_core::search::SearchResult,
    )> = results
        .into_iter()
        .map(|r| {
            let cand = result_candidate(&r);
            let quality = iris_media::filename::parse(&r.title).and_then(|p| p.quality);
            let codec = detect_codec(&r.title);
            let qr = tag_rank(quality.as_deref(), q_pref);
            let cr = tag_rank((codec != Codec::Unknown).then(|| codec.as_str()), c_pref);
            (cand, qr, cr, r)
        })
        .collect();
    // Same order as `offer_cmp`: junk guard → format → codec →
    // aliveness → recommended policy.
    decorated.sort_by(|a, b| {
        b.0.big_enough()
            .cmp(&a.0.big_enough())
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
            .then_with(|| b.0.alive().cmp(&a.0.alive()))
            .then_with(|| iris_core::ranking::recommended_cmp(&a.0, &b.0))
    });
    let tagged: Vec<(Language, bool, iris_core::search::SearchResult)> = decorated
        .into_iter()
        .map(|(cand, _, _, r)| (detect_language(&r.title), cand.sane(), r))
        .collect();
    let best = select_by_lang(&tagged, sel)?;
    let lang = detect_language(&best.title);
    let absolute_episode = iris_media::filename::parse(&best.title)
        .as_ref()
        .and_then(iris_media::filename::absolute_from_parsed)
        .map(i64::from);
    let _ = iris_db::available_episodes::upsert(
        state.db(),
        iris_db::available_episodes::UpsertAvailableEpisode {
            normalized_name: normalized_name.to_string(),
            season,
            episode,
            indexer_provider: best.provider_id.clone(),
            indexer_torrent_id: best.external_id.clone(),
            magnet: String::new(),
            quality: None,
            seeders: best.seeders.map(i64::from),
            size_bytes: best.size_bytes.map(|s| s as i64),
            language: Some(lang.as_str().to_string()),
            download_url: best.download_url.clone(),
            absolute_episode,
            codec: Some(detect_codec(&best.title).as_str().to_string()),
        },
    )
    .await;
    Some(PickedAvailability {
        magnet: String::new(),
        indexer_provider: best.provider_id,
        indexer_torrent_id: best.external_id,
        download_url: best.download_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefer_walk_skips_dodgy_preferred_language() {
        // Near-dead FR (preferred) must lose to a healthy EN.
        let items = vec![
            (Language::French, false, "fr"),
            (Language::English, true, "en"),
        ];
        let sel = LangSel::Prefer(vec![Language::French, Language::English]);
        assert_eq!(select_by_lang(&items, &sel), Some("en"));
    }

    #[test]
    fn prefer_accepts_dodgy_when_nothing_sane_exists() {
        let items = vec![(Language::French, false, "fr")];
        let sel = LangSel::Prefer(vec![Language::English, Language::French]);
        assert_eq!(select_by_lang(&items, &sel), Some("fr"));
    }

    #[test]
    fn exact_never_crosses_language_even_for_sanity() {
        let items = vec![
            (Language::English, true, "en"),
            (Language::French, false, "fr"),
        ];
        assert_eq!(
            select_by_lang(&items, &LangSel::Exact(Language::French)),
            Some("fr"),
        );
        assert_eq!(
            select_by_lang(&items, &LangSel::Exact(Language::Multi)),
            None,
        );
    }

    #[test]
    fn any_prefers_sane_over_first() {
        let items = vec![
            (Language::French, false, "fr"),
            (Language::English, true, "en"),
        ];
        assert_eq!(select_by_lang(&items, &LangSel::Any), Some("en"));
    }

    #[test]
    fn continuation_ladder_falls_back_multi_then_fr_then_en() {
        let ladder = |dominant| {
            let LangSel::Prefer(order) = continuation_pref(dominant) else {
                panic!("continuation_pref must build a Prefer");
            };
            order
        };
        assert_eq!(
            ladder(Language::Multi),
            vec![Language::Multi, Language::French, Language::English],
        );
        assert_eq!(
            ladder(Language::French),
            vec![Language::French, Language::Multi, Language::English],
        );
        assert_eq!(
            ladder(Language::English),
            vec![Language::English, Language::Multi, Language::French],
        );
    }

    #[test]
    fn tag_rank_orders_match_untagged_mismatch() {
        assert_eq!(tag_rank(Some("1080p"), Some("1080p")), 0);
        assert_eq!(tag_rank(Some("1080P"), Some("1080p")), 0);
        assert_eq!(tag_rank(None, Some("1080p")), 1);
        assert_eq!(tag_rank(Some("2160p"), Some("1080p")), 2);
        // No preference: everything is equally fine.
        assert_eq!(tag_rank(Some("2160p"), None), 0);
        assert_eq!(tag_rank(None, None), 0);
    }

    const GIB: i64 = 1_073_741_824;

    fn offer(
        seeders: i64,
        size_gib: i64,
        quality: &str,
        codec: &str,
    ) -> iris_db::available_episodes::AvailableEpisodeRow {
        iris_db::available_episodes::AvailableEpisodeRow {
            id: Uuid::new_v4(),
            normalized_name: "test".into(),
            season: 4,
            episode: 1,
            indexer_provider: "test".into(),
            indexer_torrent_id: quality.into(),
            magnet: String::new(),
            quality: Some(quality.into()),
            seeders: Some(seeders),
            size_bytes: Some(size_gib * GIB),
            found_at: Utc::now(),
            language: Some("french".into()),
            download_url: None,
            absolute_episode: None,
            codec: Some(codec.into()),
        }
    }

    fn profile_1080_hevc() -> GrabProfile {
        GrabProfile {
            quality: Some("1080p".into()),
            codec: Some("hevc".into()),
        }
    }

    #[test]
    fn quality_match_beats_smaller_mismatch() {
        // 8 GiB 1080p (profile match) must rank ahead of a lighter 720p.
        let matching = offer(50, 8, "1080p", "hevc");
        let lighter_mismatch = offer(50, 4, "720p", "hevc");
        let p = profile_1080_hevc();
        assert_eq!(offer_cmp(&matching, &lighter_mismatch, &p), Ordering::Less,);
    }

    #[test]
    fn quality_match_beats_healthy_2160p_even_when_low_seeded() {
        // The user's rule: 2160p has no business beating 1080p when the
        // series is established in 1080p — a slow (1 seeder, still
        // alive) 1080p download beats a well-seeded 4K monster.
        let slow_match = offer(1, 8, "1080p", "hevc");
        let healthy_mismatch = offer(200, 40, "2160p", "hevc");
        let p = profile_1080_hevc();
        assert_eq!(
            offer_cmp(&slow_match, &healthy_mismatch, &p),
            Ordering::Less
        );
    }

    #[test]
    fn junk_size_guard_outranks_quality_match() {
        // A sample-sized file tagged "1080p" must never win, even
        // against a format mismatch.
        let junk = iris_db::available_episodes::AvailableEpisodeRow {
            size_bytes: Some(10 * 1024 * 1024),
            ..offer(500, 0, "1080p", "hevc")
        };
        let real_mismatch = offer(50, 8, "720p", "h264");
        let p = profile_1080_hevc();
        assert_eq!(offer_cmp(&real_mismatch, &junk, &p), Ordering::Less);
    }

    #[test]
    fn codec_match_breaks_quality_tie() {
        // Both 1080p: the hevc release (profile codec) wins even a bit heavier.
        let hevc = offer(50, 9, "1080p", "hevc");
        let h264 = offer(50, 8, "1080p", "h264");
        let p = profile_1080_hevc();
        assert_eq!(offer_cmp(&hevc, &h264, &p), Ordering::Less);
    }

    #[test]
    fn no_profile_falls_back_to_recommended_policy() {
        // Without preferences the smallest sane release wins.
        let small = offer(50, 4, "720p", "h264");
        let big = offer(200, 8, "1080p", "hevc");
        let p = GrabProfile {
            quality: None,
            codec: None,
        };
        assert_eq!(offer_cmp(&small, &big, &p), Ordering::Less);
    }

    #[test]
    fn majority_tag_breaks_ties_deterministically() {
        let mut counts = BTreeMap::new();
        counts.insert("1080p".to_string(), 2);
        counts.insert("2160p".to_string(), 2);
        assert_eq!(majority_tag(counts), Some("1080p".to_string()));
        assert_eq!(majority_tag(BTreeMap::new()), None);
    }
}
