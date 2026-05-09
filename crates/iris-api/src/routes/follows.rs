// `tmdb_id`/`season`/`episode`/`file_idx` casts move between i64 (DB
// storage) and u32/u64 (TMDB / engine APIs). The values are bounded by
// the domain (no negative TMDB ids, no >2^31 seasons or file indices) so
// clippy's pedantic cast warnings on these conversions are noise.
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
)]

//! Per-user series-following endpoints. Mounted under `/api/me/follows`.
//!
//! These are the data plumbing for the Watchlist shelf and the Series
//! detail page: the user follows a TMDB id, we remember it, and we
//! cross-reference what's downloaded (`episode_files`) and what's
//! available to grab (`available_episodes`) per episode TMDB tells us
//! exists.

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::AuthUser;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/episode-context", get(episode_context))
        .route("/{tmdb_id}", delete(remove))
        .route("/{tmdb_id}/episodes", get(episodes))
        .route(
            "/{tmdb_id}/episodes/{season}/{episode}/grab",
            post(grab_episode),
        )
}

// ---------------------------------------------------------------------------
// POST /api/me/follows
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CreateFollowRequest {
    tmdb_id: i64,
    /// Snapshot of the show name. Caller (the discovery / search UI) has
    /// it in hand from the TMDB lookup that produced the poster they
    /// clicked, so we don't re-fetch it server-side. If absent, we'll
    /// look it up from TMDB; if TMDB is unconfigured AND no name was
    /// supplied, the request is rejected.
    name: Option<String>,
    /// Same logic — if missing we try TMDB; if TMDB is unavailable we
    /// just store NULL and the Series page renders without season tabs
    /// until the next visit (notify scheduler will fill it in).
    total_seasons: Option<i64>,
}

async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<CreateFollowRequest>,
) -> ApiResult<Json<FollowSummary>> {
    let (name, total_seasons) = if let Some(name) = body.name.clone() {
        (name, body.total_seasons)
    } else if let Some(tmdb) = state.tmdb() {
        match tmdb.lookup(body.tmdb_id as u64).await {
            Some(meta) => (meta.title, meta.number_of_seasons.map(i64::from)),
            None => {
                return Err(ApiError::BadRequest(format!(
                    "tmdb id {} not found",
                    body.tmdb_id
                )));
            }
        }
    } else {
        return Err(ApiError::BadRequest(
            "tmdb client not configured and no name supplied — provide `name` in the body".into(),
        ));
    };

    let row = iris_db::follows::add(state.db(), user.id, body.tmdb_id, &name, total_seasons)
        .await?;

    // Kick off an immediate background scan so the series page shows
    // `dispo` chips on first visit instead of waiting up to 4 h for
    // the scheduler tick. Pre-existing follows still rely on the
    // periodic scheduler. Best-effort: failures land in the warn log
    // and the scheduler will retry on its next pass.
    if let Some(tmdb) = state.tmdb().cloned() {
        let pool = state.db().clone();
        let providers = state.providers().clone();
        let row = row.clone();
        tokio::spawn(async move {
            if let Err(e) =
                crate::follows_scheduler::scan_follow(&pool, &tmdb, &providers, &row).await
            {
                tracing::warn!(
                    tmdb_id = row.tmdb_id,
                    name = %row.name,
                    error = %e,
                    "follow create: initial scan failed",
                );
            }
        });
    }

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
    tmdb_id: i64,
    name: String,
    total_seasons: Option<i64>,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    /// Count of episodes whose `available_episodes.found_at` is newer than
    /// `last_visited_at`. Drives the "X nouveaux" badge on Watchlist cards.
    new_count: i64,
    last_visited_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

async fn summarize(state: &AppState, row: &iris_db::follows::FollowRow) -> FollowSummary {
    // Poster + backdrop come from TMDB metadata cache. Cheap after the
    // first hit per (tmdb_id) per uptime.
    let (poster_path, backdrop_path) = if let Some(tmdb) = state.tmdb() {
        match tmdb.lookup(row.tmdb_id as u64).await {
            Some(m) => (m.poster_path, m.backdrop_path),
            None => (None, None),
        }
    } else {
        (None, None)
    };
    // Best-effort — count failures shouldn't poison the list response.
    let new_count = iris_db::available_episodes::count_new_for_series(
        state.db(),
        row.tmdb_id,
        row.last_visited_at,
    )
    .await
    .unwrap_or(0);
    FollowSummary {
        tmdb_id: row.tmdb_id,
        name: row.name.clone(),
        total_seasons: row.total_seasons,
        poster_path,
        backdrop_path,
        new_count,
        last_visited_at: row.last_visited_at,
        created_at: row.created_at,
    }
}

// ---------------------------------------------------------------------------
// DELETE /api/me/follows/:tmdb_id
// ---------------------------------------------------------------------------

async fn remove(
    State(state): State<AppState>,
    user: AuthUser,
    Path(tmdb_id): Path<i64>,
) -> ApiResult<StatusCode> {
    let removed = iris_db::follows::delete(state.db(), user.id, tmdb_id).await?;
    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

// ---------------------------------------------------------------------------
// GET /api/me/follows/:tmdb_id/episodes?season=N
// ---------------------------------------------------------------------------
//
// Fetching this endpoint counts as a "visit" — `last_visited_at` is
// bumped, which resets the "X nouveaux" badge. The Series page hits this
// on mount, so the badge clearing is naturally in lockstep with the user
// actually seeing what's new.

#[derive(Debug, Deserialize)]
struct EpisodesQuery {
    /// Defaults to season 1 if omitted. The caller (Series page) drives
    /// per-season tabs and re-fetches as the user clicks across.
    season: Option<u32>,
}

async fn episodes(
    State(state): State<AppState>,
    user: AuthUser,
    Path(tmdb_id): Path<i64>,
    Query(q): Query<EpisodesQuery>,
) -> ApiResult<Json<EpisodesResponse>> {
    let follow = iris_db::follows::get(state.db(), user.id, tmdb_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let season = q.season.unwrap_or(1);

    let tmdb = state
        .tmdb()
        .ok_or_else(|| ApiError::BadRequest("tmdb client not configured".into()))?;

    let expected = tmdb.tv_season_episodes(tmdb_id as u64, season).await;
    let downloaded = iris_db::episode_files::list_for_season(state.db(), tmdb_id, i64::from(season))
        .await
        .unwrap_or_default();
    // Future: could filter `available_episodes` to this season too, but
    // the table's small enough that grabbing the whole series and
    // filtering in memory is fine.
    let available = iris_db::available_episodes::list_best_for_series(state.db(), tmdb_id)
        .await
        .unwrap_or_default();

    // Watched lookup: per-user. Pull the user's playback rows for each
    // downloaded episode's torrent. Cheap because we only ever ask about
    // episodes we already have on disk.
    let mut watched_keys = std::collections::HashSet::new();
    for d in &downloaded {
        let p = iris_db::playback::get(state.db(), user.id, &d.infohash, d.file_idx)
            .await
            .unwrap_or(None);
        if let Some(p) = p {
            if p.completed {
                watched_keys.insert((d.season, d.episode));
            }
        }
    }

    let items = expected
        .into_iter()
        .map(|e| {
            let dl = downloaded
                .iter()
                .find(|d| d.season == i64::from(e.season) && d.episode == i64::from(e.episode));
            let av = available
                .iter()
                .find(|a| a.season == i64::from(e.season) && a.episode == i64::from(e.episode));
            let watched = watched_keys.contains(&(i64::from(e.season), i64::from(e.episode)));
            let status = if dl.is_some() {
                EpisodeStatus::Downloaded
            } else if av.is_some() {
                EpisodeStatus::Available
            } else {
                EpisodeStatus::Unavailable
            };
            EpisodeItem {
                season: e.season,
                episode: e.episode,
                name: e.name,
                overview: e.overview,
                air_date: e.air_date,
                still_path: e.still_path,
                runtime_minutes: e.runtime_minutes,
                status,
                watched,
                infohash: dl.map(|d| d.infohash.clone()),
                file_idx: dl.map(|d| d.file_idx),
                indexer_provider: av.map(|a| a.indexer_provider.clone()),
                indexer_torrent_id: av.map(|a| a.indexer_torrent_id.clone()),
            }
        })
        .collect();

    // Bump visited timestamp AFTER we've used the previous value (we don't
    // need it past this point; the response carries the episode statuses
    // already). Failures here aren't worth aborting on.
    let _ = iris_db::follows::mark_visited(state.db(), user.id, tmdb_id).await;

    Ok(Json(EpisodesResponse {
        season,
        total_seasons: follow.total_seasons,
        items,
    }))
}

#[derive(Debug, Serialize)]
struct EpisodesResponse {
    season: u32,
    total_seasons: Option<i64>,
    items: Vec<EpisodeItem>,
}

#[derive(Debug, Serialize)]
struct EpisodeItem {
    season: u32,
    episode: u32,
    name: Option<String>,
    overview: Option<String>,
    air_date: Option<String>,
    still_path: Option<String>,
    runtime_minutes: Option<u32>,
    status: EpisodeStatus,
    /// Per-user `playback_progress.completed` for the underlying file (only
    /// meaningful when status == Downloaded).
    watched: bool,
    /// Set when status == Downloaded — what to navigate to for play.
    infohash: Option<String>,
    file_idx: Option<i64>,
    /// Set when status == Available — what to pass to the on-demand grab
    /// endpoint (Phase 4).
    indexer_provider: Option<String>,
    indexer_torrent_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum EpisodeStatus {
    Downloaded,
    Available,
    Unavailable,
}

// ---------------------------------------------------------------------------
// GET /api/me/follows/episode-context?infohash=X&file_idx=N
// ---------------------------------------------------------------------------
//
// Used by the player at end-of-episode to decide whether to prompt
// "Préparer le suivant ?". Bundles the three lookups (episode_files →
// follow → next episode + status) into one round-trip so the player
// doesn't have to issue them sequentially.
//
// Returns nulls (not 404) when the file isn't part of a followed series
// — the caller treats absence as "no prompt needed", which is the
// dominant case (most plays aren't TV episodes).

#[derive(Debug, Deserialize)]
struct EpisodeContextParams {
    infohash: String,
    file_idx: i64,
}

#[derive(Debug, Serialize)]
struct EpisodeContext {
    followed: bool,
    current: Option<EpisodePoint>,
    /// Set only when followed AND we found a meaningful "next" episode.
    /// `next.status` mirrors the Series page status enum so the caller
    /// can render the modal exactly when status == "available".
    next: Option<EpisodePoint>,
}

#[derive(Debug, Serialize)]
struct EpisodePoint {
    tmdb_id: i64,
    season: i64,
    episode: i64,
    status: EpisodeStatus,
}

async fn episode_context(
    State(state): State<AppState>,
    user: AuthUser,
    Query(p): Query<EpisodeContextParams>,
) -> ApiResult<Json<EpisodeContext>> {
    // Step 1: which episode IS this file?
    let candidates: Vec<iris_db::episode_files::EpisodeFileRow> = sqlx::query_as(
        "SELECT id, tmdb_id, season, episode, infohash, file_idx, derived_from, created_at \
         FROM episode_files WHERE infohash = ?1 AND file_idx = ?2",
    )
    .bind(&p.infohash)
    .bind(p.file_idx)
    .fetch_all(state.db())
    .await?;
    let Some(current_row) = candidates.into_iter().next() else {
        return Ok(Json(EpisodeContext {
            followed: false,
            current: None,
            next: None,
        }));
    };

    // Step 2: is the user following this series?
    let follow = iris_db::follows::get(state.db(), user.id, current_row.tmdb_id).await?;
    let followed = follow.is_some();

    let current = EpisodePoint {
        tmdb_id: current_row.tmdb_id,
        season: current_row.season,
        episode: current_row.episode,
        status: EpisodeStatus::Downloaded,
    };

    if !followed {
        return Ok(Json(EpisodeContext {
            followed: false,
            current: Some(current),
            next: None,
        }));
    }

    // Step 3: find the "next" episode. Same season + 1; if absent in
    // TMDB's list, try (season+1, 1). We don't go further than that —
    // a missing N+2 means the show is over or hasn't aired more.
    let tmdb = state.tmdb();
    let same_season_eps = if let Some(t) = tmdb {
        t.tv_season_episodes(current_row.tmdb_id as u64, current_row.season as u32)
            .await
    } else {
        Vec::new()
    };
    let same_season_has_next = same_season_eps
        .iter()
        .any(|e| i64::from(e.episode) == current_row.episode + 1);

    let (next_season, next_episode) = if same_season_has_next {
        (current_row.season, current_row.episode + 1)
    } else {
        // End-of-season jump.
        let next_season_num = current_row.season + 1;
        let next_eps = if let Some(t) = tmdb {
            t.tv_season_episodes(current_row.tmdb_id as u64, next_season_num as u32)
                .await
        } else {
            Vec::new()
        };
        if next_eps.is_empty() {
            return Ok(Json(EpisodeContext {
                followed: true,
                current: Some(current),
                next: None,
            }));
        }
        (next_season_num, 1)
    };

    // Compute status of the next: downloaded / available / unavailable.
    let downloaded = iris_db::episode_files::list_for_season(
        state.db(),
        current_row.tmdb_id,
        next_season,
    )
    .await
    .unwrap_or_default()
    .into_iter()
    .any(|r| r.episode == next_episode);

    let status = if downloaded {
        EpisodeStatus::Downloaded
    } else {
        let avail_rows = iris_db::available_episodes::list_best_for_series(
            state.db(),
            current_row.tmdb_id,
        )
        .await
        .unwrap_or_default();
        if avail_rows
            .iter()
            .any(|a| a.season == next_season && a.episode == next_episode)
        {
            EpisodeStatus::Available
        } else {
            EpisodeStatus::Unavailable
        }
    };

    Ok(Json(EpisodeContext {
        followed: true,
        current: Some(current),
        next: Some(EpisodePoint {
            tmdb_id: current_row.tmdb_id,
            season: next_season,
            episode: next_episode,
            status,
        }),
    }))
}

// ---------------------------------------------------------------------------
// POST /api/me/follows/:tmdb_id/episodes/:season/:episode/grab
// ---------------------------------------------------------------------------
//
// Triggered by:
//   * the "Lire" button on an episode the user hasn't downloaded yet
//   * the "Préparer le suivant ?" modal at the end of an episode
//   * the "Préparer" button (just-fetch-without-playing variant)
//
// The endpoint is idempotent — if the episode is already in
// `episode_files` we return its existing (infohash, file_idx) so the
// caller can navigate straight to /watch.

#[derive(Debug, Serialize)]
struct GrabResponse {
    infohash: String,
    file_idx: i64,
    /// True when the episode was already in the library (idempotent
    /// no-op). False when this call actually triggered an ingest.
    already_grabbed: bool,
}

async fn grab_episode(
    State(state): State<AppState>,
    user: AuthUser,
    Path((tmdb_id, season, episode)): Path<(i64, i64, i64)>,
) -> ApiResult<Json<GrabResponse>> {
    // Caller must follow the show — keeps the grab endpoint scoped to
    // intentional user behaviour and avoids strangers triggering ingests
    // by guessing tmdb_ids.
    let follow = iris_db::follows::get(state.db(), user.id, tmdb_id)
        .await?
        .ok_or(ApiError::NotFound)?;

    // Idempotent shortcut: episode already on disk.
    if let Some(existing) = find_episode_file(state.db(), tmdb_id, season, episode).await? {
        return Ok(Json(GrabResponse {
            infohash: existing.infohash,
            file_idx: existing.file_idx,
            already_grabbed: true,
        }));
    }

    // Pick the best magnet: prefer a pre-cached `available_episodes`
    // row (populated by the notify scheduler), otherwise hit the indexer
    // inline so a click never gates on a periodic background task.
    let pick = match best_available(state.db(), tmdb_id, season, episode).await? {
        Some(p) => p,
        None => find_via_indexer(&state, tmdb_id, &follow.name, season, episode)
            .await?
            .ok_or(ApiError::NotFound)?,
    };

    // Resolve the picked source. The cache may carry an empty magnet
    // when the scheduler couldn't materialise one (provider returns
    // `.torrent` files, not magnets — torr9's case). In that fallback
    // we re-resolve through the provider here, which gives us either
    // a fresh magnet OR the .torrent bytes. Both routes plug into the
    // engine.
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

    // Mirror the ingest route's DB upsert so the torrent shows up in
    // /api/torrents and benefits from the regular GC + remux pipeline.
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
            tmdb_id: Some(tmdb_id),
            added_by: user.id,
        },
    )
    .await?;

    // Pick the file representing the episode. For the on-demand grab
    // path the magnet was found by searching for "Show SXXEXX" — single-
    // episode releases dominate, so the largest video file is reliably
    // the right one. Season packs would need filename-parsing to pick
    // the right episode (handled in P4.5 collection assignment).
    let video_exts = [
        "mkv", "mp4", "webm", "m4v", "avi", "mov", "ts", "mts", "m2ts", "wmv",
    ];
    let file_idx = result
        .snapshot
        .files
        .iter()
        .filter(|f| {
            std::path::Path::new(&f.path)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| video_exts.contains(&e.to_ascii_lowercase().as_str()))
        })
        .max_by_key(|f| f.size_bytes)
        .map_or(0, |f| f.index as i64);

    // Record the (tmdb_id, season, episode) → (infohash, file_idx)
    // mapping so the Series page picks it up immediately AND future
    // grab calls short-circuit through the idempotent path above.
    let _ = iris_db::episode_files::upsert(
        state.db(),
        iris_db::episode_files::UpsertEpisodeFile {
            tmdb_id,
            season,
            episode,
            infohash: result.snapshot.infohash.clone(),
            file_idx,
            derived_from: iris_db::episode_files::DerivedFrom::TmdbMatch,
        },
    )
    .await;

    // Collection assignment — same as the regular ingest route. Done
    // synchronously here (not spawned) because the Series page relies
    // on the collection link being there for the next render.
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
        Some(tmdb_id),
        &files,
    )
    .await;

    Ok(Json(GrabResponse {
        infohash: result.snapshot.infohash,
        file_idx,
        already_grabbed: result.already_managed,
    }))
}

/// Existing `episode_files` row for `(tmdb_id, season, episode)`. We
/// don't have a query-by-key in the DB module yet, so filter the
/// `list_for_season` result.
async fn find_episode_file(
    pool: &iris_db::SqlitePool,
    tmdb_id: i64,
    season: i64,
    episode: i64,
) -> Result<Option<iris_db::episode_files::EpisodeFileRow>, sqlx::Error> {
    let rows = iris_db::episode_files::list_for_season(pool, tmdb_id, season).await?;
    Ok(rows.into_iter().find(|r| r.episode == episode))
}

/// Best `available_episodes` candidate for `(tmdb_id, season, episode)`.
/// Picks highest seeders. Returned tuple matches the shape needed by the
/// ingest path (magnet + provenance for the torrents table).
struct PickedAvailability {
    magnet: String,
    indexer_provider: String,
    indexer_torrent_id: String,
}

async fn best_available(
    pool: &iris_db::SqlitePool,
    tmdb_id: i64,
    season: i64,
    episode: i64,
) -> Result<Option<PickedAvailability>, sqlx::Error> {
    let rows = iris_db::available_episodes::list_best_for_series(pool, tmdb_id).await?;
    Ok(rows
        .into_iter()
        .find(|r| r.season == season && r.episode == episode)
        .map(|r| PickedAvailability {
            magnet: r.magnet,
            indexer_provider: r.indexer_provider,
            indexer_torrent_id: r.indexer_torrent_id,
        }))
}

/// Inline indexer fallback when the notify scheduler hasn't (yet) cached
/// an `available_episodes` row for this `(tmdb_id, season, episode)`.
/// Builds a SCENE-style query, picks the best candidate by seeders, and
/// resolves the magnet through the provider's `resolve` path.
async fn find_via_indexer(
    state: &AppState,
    tmdb_id: i64,
    series_name: &str,
    season: i64,
    episode: i64,
) -> Result<Option<PickedAvailability>, ApiError> {
    use iris_core::search::{SearchQuery, SortField, SortOrder};
    let query = SearchQuery {
        q: format!("{series_name} S{season:02}E{episode:02}"),
        page: Some(1),
        limit: Some(20),
        sort_by: Some(SortField::Seeders),
        order: Some(SortOrder::Desc),
        kind: None,
    };
    let agg = state.providers().search_all(&query).await;
    // Pick the candidate with the most seeders — providers already sort
    // by seeders (we asked) but the multi-provider fan-out concatenates,
    // so re-sort defensively.
    let mut sorted = agg.results;
    sorted.sort_by_key(|r| std::cmp::Reverse(r.seeders.unwrap_or(0)));
    let Some(best) = sorted.into_iter().next() else {
        return Ok(None);
    };

    // Don't resolve here — same rationale as the scheduler: providers
    // that hand back `.torrent` files would force us to either store
    // opaque bytes in the cache or skip them. The grab path already
    // handles "empty magnet → re-resolve through provider" so we just
    // record the indexer ref and let the caller resolve.
    let _ = iris_db::available_episodes::upsert(
        state.db(),
        iris_db::available_episodes::UpsertAvailableEpisode {
            tmdb_id,
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
