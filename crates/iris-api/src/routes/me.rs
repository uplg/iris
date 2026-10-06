use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use chrono::{DateTime, Utc};
use iris_core::search::MediaKind;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::routes::PageQuery;
use crate::routes::extract::AuthUser;
use crate::routes::library;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(me))
        .route("/continue-watching", get(continue_watching))
        .route(
            "/continue-watching/dismiss",
            axum::routing::post(dismiss_continue_watching),
        )
        .route("/history", get(history))
        .route("/gone/dismiss", axum::routing::post(dismiss_gone))
        .route("/summary", get(summary))
        .route(
            "/recent-searches",
            get(recent_searches)
                .post(record_search)
                .delete(forget_searches),
        )
        .route("/watchlist", get(watchlist))
        .route("/watchlist/remove", axum::routing::post(remove_watchlist))
        .route("/password", axum::routing::post(change_password))
        .route("/display-name", axum::routing::post(change_display_name))
}

#[derive(Debug, serde::Deserialize, ToSchema)]
pub(crate) struct ChangePasswordRequest {
    pub old_password: String,
    pub new_password: String,
}

#[utoipa::path(
    post,
    path = "/api/me/password",
    operation_id = "change_password",
    request_body = ChangePasswordRequest,
    responses(
        (status = 204, description = "Password changed; every session revoked, this one included (its cookies are cleared), and every passkey removed"),
        (status = 400, description = "New password too short (min 8 chars), or the current one is wrong"),
        (status = 401, description = "Not authenticated"),
    ),
    tag = "me",
)]
pub(crate) async fn change_password(
    State(state): State<AppState>,
    jar: axum_extra::extract::CookieJar,
    user: AuthUser,
    Json(body): Json<ChangePasswordRequest>,
) -> ApiResult<(axum_extra::extract::CookieJar, axum::http::StatusCode)> {
    crate::passwords::check_policy(&body.new_password)?;
    let current = iris_db::users::get_password_hash(state.db(), user.id)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    // 400, not 401: a wrong current password is a form error, not an
    // expired session (a 401 makes clients try a refresh first).
    if !crate::passwords::verify(&body.old_password, &current).await? {
        return Err(ApiError::Invalid {
            code: "wrong_password",
            message: "This is not your current password.".into(),
        });
    }
    let new_hash = crate::passwords::hash(&body.new_password).await?;
    // Every session ends with the old password, this one included: the
    // caller signs in again with the new one.
    let passkeys = iris_db::users::set_password(state.db(), user.id, &new_hash)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    let id = Uuid::from(user.id).to_string();
    super::audit(
        &state,
        user.id,
        "user.password_change",
        "user",
        Some(&id),
        None,
    )
    .await;
    if passkeys > 0 {
        super::audit(
            &state,
            user.id,
            "user.passkeys_revoked",
            "user",
            Some(&id),
            Some(&format!("{passkeys} removed by the password change")),
        )
        .await;
    }
    Ok((
        crate::routes::auth::clear_session(jar),
        axum::http::StatusCode::NO_CONTENT,
    ))
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct MeResponse {
    id: Uuid,
    email: String,
    display_name: String,
    is_admin: bool,
}

#[utoipa::path(
    get,
    path = "/api/me",
    operation_id = "get_me",
    responses(
        (status = 200, description = "The authenticated user's profile", body = MeResponse),
        (status = 401, description = "Not authenticated"),
    ),
    tag = "me",
)]
pub(crate) async fn me(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<MeResponse>> {
    let u = iris_db::users::find_by_id(state.db(), user.id)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    Ok(Json(MeResponse {
        id: u.id.into(),
        email: u.email,
        display_name: u.display_name,
        is_admin: u.is_admin,
    }))
}

#[derive(Debug, serde::Deserialize, ToSchema)]
pub(crate) struct ChangeDisplayNameRequest {
    pub display_name: String,
}

#[utoipa::path(
    post,
    path = "/api/me/display-name",
    operation_id = "change_display_name",
    request_body = ChangeDisplayNameRequest,
    responses(
        (status = 204, description = "Display name updated"),
        (status = 400, description = "Empty / too-long display name (max 64)"),
    ),
    tag = "me",
)]
pub(crate) async fn change_display_name(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<ChangeDisplayNameRequest>,
) -> ApiResult<axum::http::StatusCode> {
    let trimmed = checked_display_name(&body.display_name)?;
    let _ = iris_db::users::update_display_name(state.db(), user.id, trimmed).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// A display name as stored: trimmed, non-empty, at most 64 bytes.
pub(crate) fn checked_display_name(raw: &str) -> ApiResult<&str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("display name cannot be empty".into()));
    }
    if trimmed.len() > 64 {
        return Err(ApiError::BadRequest(
            "display name too long (max 64)".into(),
        ));
    }
    Ok(trimmed)
}

// Wire shape: the bools are independent per-tile flags, not a state
// machine to encode.
#[expect(clippy::struct_excessive_bools)]
#[derive(Debug, serde::Serialize, ToSchema)]
pub(crate) struct ContinueWatchingItem {
    infohash: String,
    torrent_name: String,
    tmdb_id: Option<i64>,
    tmdb_verified: bool,
    /// `"movie"` / `"tv"` from the parent collection. Clients pass
    /// this to `/api/metadata/tmdb/{id}?kind=` — without it, TMDB's
    /// separate id namespaces collide and the lookup serves a
    /// stranger's poster.
    kind: Option<MediaKind>,
    file_idx: i64,
    file_path: Option<String>,
    position_seconds: f64,
    duration_seconds: Option<f64>,
    last_watched_at: chrono::DateTime<chrono::Utc>,
    completed: bool,
    /// Parent collection id (TV series). Sent back to
    /// `/api/me/continue-watching/dismiss` so "remove" hides the whole
    /// series, not just one episode. Null for movies / standalone.
    collection_id: Option<uuid::Uuid>,
    /// True when this is the NEXT unstarted episode (the previous one is
    /// finished) rather than a mid-way resume. Clients label it "Up next".
    next_up: bool,
    /// `(season, episode)` of the tile when known — the `episode_files`
    /// mapping for resume tiles, the candidate itself for next-up tiles.
    /// Render "S08E08" from these instead of SCENE-parsing file names.
    season: Option<i64>,
    episode: Option<i64>,
    /// TMDB's episode title, when known. Additive.
    #[serde(default)]
    episode_name: Option<String>,
    /// TMDB poster path once `tmdb_verified`. Additive.
    #[serde(default)]
    poster_path: Option<String>,
    /// True when the tile's target is NOT playable from disk: the next
    /// episode exists (per TMDB, aired) but was never downloaded, or the
    /// previously-owned file is gone (GC-reclaimed, or lost by the
    /// engine). `infohash` is empty and `file_idx` meaningless. Clients
    /// show a grab affordance and call
    /// `POST /api/library/collections/{collection_id}/grab/{season}/{episode}?language=auto`,
    /// then play the returned `(infohash, file_idx)`. Only ever true when
    /// the request opted in via `include_grabbable`.
    grabbable: bool,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ContinueWatchingQuery {
    /// Opt-in to synthesised "grab the next episode" tiles. Off by
    /// default so clients shipped before the field existed never receive
    /// rows they'd try (and fail) to play directly.
    #[serde(default)]
    include_grabbable: bool,
}

/// Post-0.4 "My Watchlist" payload. Derived from TV collections
/// that have at least one ingested episode — the household
/// auto-tracks every show they're watching, no Follow button. The
/// shape mirrors what the legacy `/api/me/follows` façade returns
/// so the web client can flip endpoints without rewriting card
/// rendering. Old APK 0.3.1 keeps calling `/api/me/follows`.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct WatchlistItem {
    /// Collection id — clients route to `/collection/:id`.
    id: Uuid,
    /// SCENE-normalised name. Clients use this to detect "is this
    /// search result already on my Watchlist?" without having to
    /// run the same normaliser themselves.
    normalized_name: String,
    name: String,
    tmdb_id: Option<i64>,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    /// Distinct (season, episode) the indexer has surfaced since
    /// the requesting user last opened this collection. Drives the
    /// "X new" tile badge.
    new_count: i64,
    last_visited_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

#[utoipa::path(
    get,
    path = "/api/me/watchlist",
    operation_id = "list_watchlist",
    responses(
        (status = 200, description = "The caller's per-user Watchlist (TV collections)", body = [WatchlistItem]),
        (status = 401, description = "Not authenticated"),
    ),
    tag = "me",
)]
pub(crate) async fn watchlist(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<WatchlistItem>>> {
    // Per-user: this household now has ~10 viewers from different
    // families and "what's on the Watchlist" is personal. The shared
    // surface is the library (episode_files / available_episodes —
    // disk content is one copy for everyone); the per-user state
    // lives in `series_follows`, auto-created when the user grabs
    // or plays an episode (see `grab_episode_core`). The collection
    // it joins through is shared, so we still surface the same
    // poster + display title for everyone.
    let follows = iris_db::follows::list_for_user(state.db(), user.id).await?;
    let out = crate::fanout::map_ordered(follows, |f| watchlist_item(&state, user.id, f)).await;
    Ok(Json(out))
}

async fn watchlist_item(
    state: &AppState,
    user_id: iris_core::ids::UserId,
    f: iris_db::follows::FollowRow,
) -> WatchlistItem {
    // Each follow joins through its normalised name to a
    // (maybe present, maybe verified) TV collection so the tile
    // can borrow the canonical title + poster. Missing collection
    // = the user has a follow but no episodes ingested yet —
    // surface the follow's own name and let the poster slot stay
    // empty.
    let collection = iris_db::collections::find_by_parsed_title(
        state.db(),
        &f.normalized_name,
        iris_db::collections::Kind::Tv,
    )
    .await
    .unwrap_or(None);
    let (display_title, tmdb_id, collection_id) = match collection {
        Some(c) => (c.display_title, c.tmdb_id.or(f.tmdb_id), c.id),
        // No collection yet → route the tile to a hypothetical
        // collection path. The user will see the empty-state
        // until first ingest; this stays consistent with the
        // collection routing the rest of the UI uses.
        None => (f.name.clone(), f.tmdb_id, f.id),
    };
    // Watchlist is TV-only by construction (we derive it from
    // `series_follows`). Hint the TMDB namespace so the same
    // numerical id can't collide with an unrelated movie.
    let (poster_path, backdrop_path) = library::collection_artwork(state, tmdb_id, "tv").await;
    // "New" cutoff = last ENGAGEMENT (max of page visit and watch)
    // — visit-only kept badging episodes that were already out when
    // the user watched, and badged the whole cache when they had
    // never opened the page. (With no collection resolved,
    // `collection_id` is the follow's id → lookup returns None.)
    let last_watched =
        iris_db::playback::last_watched_in_collection(state.db(), user_id, collection_id)
            .await
            .unwrap_or(None);
    let engaged_at = library::engaged_at(f.last_visited_at, last_watched);
    let new_count = iris_db::available_episodes::count_new_for_series(
        state.db(),
        &f.normalized_name,
        engaged_at,
    )
    .await
    .unwrap_or(0);
    WatchlistItem {
        id: collection_id,
        normalized_name: f.normalized_name,
        name: display_title,
        tmdb_id,
        poster_path,
        backdrop_path,
        new_count,
        last_visited_at: f.last_visited_at,
        created_at: f.created_at,
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct RecentSearchView {
    query: String,
    searched_at: DateTime<Utc>,
}

#[utoipa::path(
    get,
    path = "/api/me/recent-searches",
    responses((status = 200, description = "The account's last searches, newest first", body = [RecentSearchView])),
    tag = "me",
)]
pub(crate) async fn recent_searches(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<RecentSearchView>>> {
    let rows = iris_db::recent_searches::list(state.db(), user.id).await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| RecentSearchView {
                query: r.query,
                searched_at: r.searched_at,
            })
            .collect(),
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct RecordSearchRequest {
    query: String,
}

#[utoipa::path(
    post,
    path = "/api/me/recent-searches",
    request_body = RecordSearchRequest,
    responses((status = 204), (status = 400, description = "Empty or too long")),
    tag = "me",
)]
pub(crate) async fn record_search(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<RecordSearchRequest>,
) -> ApiResult<axum::http::StatusCode> {
    let query = req.query.trim();
    if query.chars().count() < 2 || query.len() > 200 {
        return Err(ApiError::BadRequest(
            "a search is 2 to 200 characters".into(),
        ));
    }
    iris_db::recent_searches::record(state.db(), user.id, query).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct ForgetSearchParams {
    /// The search to forget; every search when absent.
    q: Option<String>,
}

#[utoipa::path(
    delete,
    path = "/api/me/recent-searches",
    params(ForgetSearchParams),
    responses((status = 204)),
    tag = "me",
)]
pub(crate) async fn forget_searches(
    State(state): State<AppState>,
    user: AuthUser,
    axum::extract::Query(params): axum::extract::Query<ForgetSearchParams>,
) -> ApiResult<axum::http::StatusCode> {
    iris_db::recent_searches::forget(state.db(), user.id, params.q.as_deref()).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// The "right now" line of the home page: disk, downloads, seeding, new
/// episodes. For every account (the admin storage view is quota-based and
/// admin-only).
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct HomeSummary {
    /// The filesystem holding the downloads; `None` when it can't be read.
    disk: Option<DiskSpace>,
    /// Torrents still downloading.
    downloading: u32,
    /// Overall progress of those downloads, by bytes (0–100).
    downloading_pct: f64,
    /// Seconds until they all finish at the current speed; `None` when
    /// nothing moves.
    downloading_eta_seconds: Option<u64>,
    /// Finished torrents still sharing with the swarm.
    seeding: u32,
    /// New episodes across the caller's watchlist, as its badges count them.
    new_episodes: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct DiskSpace {
    total_bytes: u64,
    free_bytes: u64,
}

#[utoipa::path(
    get,
    path = "/api/me/summary",
    responses((status = 200, body = HomeSummary)),
    tag = "me",
)]
pub(crate) async fn summary(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<HomeSummary>> {
    let torrents = state.engine().list();
    let active: Vec<_> = torrents
        .iter()
        .filter(|t| !t.finished && t.state != iris_torrent::TorrentState::Error)
        .collect();
    let total: u64 = active.iter().map(|t| t.total_size_bytes).sum();
    let done: u64 = active.iter().map(|t| t.progress_bytes).sum();
    let speed: u64 = active.iter().map(|t| t.download_speed_bps).sum();
    #[allow(clippy::cast_precision_loss)]
    let downloading_pct = if total > 0 {
        done as f64 / total as f64 * 100.0
    } else {
        0.0
    };
    let downloading_eta_seconds = (speed > 0).then(|| total.saturating_sub(done) / speed);
    let seeding = torrents
        .iter()
        .filter(|t| t.finished && t.state == iris_torrent::TorrentState::Live)
        .count();

    let follows = iris_db::follows::list_for_user(state.db(), user.id).await?;
    let state_ref = &state;
    let new_episodes =
        crate::fanout::map_ordered(follows, |f| watchlist_item(state_ref, user.id, f))
            .await
            .iter()
            .map(|w| w.new_count)
            .sum();

    let dir = state.cfg().storage.download_dir.clone();
    let disk = tokio::task::spawn_blocking(move || disk_space(&dir))
        .await
        .ok()
        .flatten();
    Ok(Json(HomeSummary {
        disk,
        downloading: u32::try_from(active.len()).unwrap_or(u32::MAX),
        downloading_pct,
        downloading_eta_seconds,
        seeding: u32::try_from(seeding).unwrap_or(u32::MAX),
        new_episodes,
    }))
}

/// Size and free space of the filesystem holding `dir`, as an unprivileged
/// user sees it.
fn disk_space(dir: &std::path::Path) -> Option<DiskSpace> {
    let st = rustix::fs::statvfs(dir).ok()?;
    let block = to_u64(st.f_frsize);
    Some(DiskSpace {
        total_bytes: to_u64(st.f_blocks).saturating_mul(block),
        free_bytes: to_u64(st.f_bavail).saturating_mul(block),
    })
}

/// `statvfs` field widths differ across platforms (`u32` on macOS, `u64`
/// or `c_long` on Linux).
fn to_u64<T: TryInto<u64>>(v: T) -> u64 {
    v.try_into().unwrap_or(0)
}

#[derive(Debug, serde::Deserialize, ToSchema)]
pub(crate) struct RemoveWatchlistRequest {
    /// The stable identity a `WatchlistItem` carries (`id` is the
    /// collection's, not the follow's).
    normalized_name: String,
}

/// Remove a series from the CALLER's Watchlist. Reversible by nature:
/// grabbing or playing an episode auto-recreates the follow.
#[utoipa::path(
    post,
    path = "/api/me/watchlist/remove",
    request_body = RemoveWatchlistRequest,
    responses(
        (status = 204, description = "Removed from the caller's Watchlist"),
        (status = 404, description = "Not on the caller's Watchlist"),
        (status = 401, description = "Not authenticated"),
    ),
    tag = "me",
)]
pub(crate) async fn remove_watchlist(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<RemoveWatchlistRequest>,
) -> ApiResult<axum::http::StatusCode> {
    let removed =
        iris_db::follows::delete_by_normalized(state.db(), user.id, &body.normalized_name).await?;
    if removed {
        Ok(axum::http::StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

#[utoipa::path(
    get,
    path = "/api/me/continue-watching",
    operation_id = "continue_watching",
    params(ContinueWatchingQuery),
    responses(
        (status = 200, description = "Recently-watched, not-yet-finished items for resume", body = [ContinueWatchingItem]),
        (status = 401, description = "Not authenticated"),
    ),
    tag = "me",
)]
pub(crate) async fn continue_watching(
    State(state): State<AppState>,
    user: AuthUser,
    axum::extract::Query(q): axum::extract::Query<ContinueWatchingQuery>,
) -> ApiResult<Json<Vec<ContinueWatchingItem>>> {
    // Two sources: episodes/movies the user paused mid-way (resume), and the
    // NEXT owned episode for any series whose most-recent episode is finished
    // (next-up). Merge them into one shelf, one tile per series — a resume
    // tile wins over a next-up tile, and within a series the most-recently
    // active wins — then trim to the shelf size.
    let resume = iris_db::playback::continue_watching(state.db(), user.id, 24).await?;
    let candidates = iris_db::playback::continue_watching_next_up(state.db(), user.id, 24).await?;
    let state_ref = &state;
    let next_up: Vec<_> = crate::fanout::map_ordered(candidates, |c| async move {
        // Cross-season candidates need TMDB to confirm the completed episode
        // really was the season finale; a same-season (e+1) never does, so
        // don't spend the lookup on it (cached, but still a cold fetch once).
        let finale = if c.next_season == c.prev_season {
            None
        } else {
            season_finale_episode(state_ref, c.row.tmdb_id, c.prev_season).await
        };
        next_up_follows_watch_order(
            (c.prev_season, c.prev_episode),
            (c.next_season, c.next_episode),
            finale,
        )
        .then_some(c.row)
    })
    .await
    .into_iter()
    .flatten()
    .collect();

    let mut merged: Vec<iris_db::playback::ContinueWatchingRow> = Vec::new();
    // collection_id → index into `merged`, so a series appears once.
    let mut by_collection: std::collections::HashMap<uuid::Uuid, usize> =
        std::collections::HashMap::new();
    // Resume rows first so they take precedence over a next-up tile for the
    // same series; both lists are already newest-first.
    for r in resume.into_iter().chain(next_up) {
        if let Some(cid) = r.collection_id {
            if let Some(&i) = by_collection.get(&cid) {
                // Keep whichever is more recent for this series.
                if r.last_watched_at > merged[i].last_watched_at {
                    merged[i] = r;
                }
                continue;
            }
            by_collection.insert(cid, merged.len());
        }
        merged.push(r);
    }

    // Owned tiles must point at a target the engine can actually serve.
    // The DB and the engine drift: the shelf may have been built moments
    // before a GC pass, a session restore can lose a torrent, and a grab
    // can die between the DB upsert and the engine add. A row whose
    // infohash the engine no longer knows navigates straight to a 404
    // player page. Convert those to grabbable tiles — same series, same
    // (S, E); the grab short-circuits to an alive copy when one exists
    // and re-fetches the release otherwise — or drop them when conversion
    // isn't possible: a missing tile beats a dead one. Dropping also
    // frees the collection slot so the TMDB frontier below can synthesise
    // its own tile for the series.
    merged.retain_mut(|r| {
        if state.engine().contains(&r.infohash) {
            return true;
        }
        if revive_dead_row(r, q.include_grabbable) {
            return true;
        }
        if let Some(cid) = r.collection_id {
            by_collection.remove(&cid);
        }
        false
    });

    if q.include_grabbable {
        append_grabbable_next_up(&state, user.id, &mut merged, &mut by_collection).await?;
    }

    merged.sort_by_key(|r| std::cmp::Reverse(r.last_watched_at));
    merged.truncate(12);

    let state = &state;
    let out = crate::fanout::map_ordered(merged, |r| async move {
        let tv = r.kind.as_deref() == Some("tv");
        let (episode_name, poster_path) = tokio::join!(
            async {
                if tv {
                    library::episode_name(state, r.tmdb_id, r.season, r.episode).await
                } else {
                    None
                }
            },
            library::verified_poster(state, r.tmdb_id, r.tmdb_verified, r.kind.as_deref())
        );
        let file_path = state.engine().file_name(&r.infohash, r.file_idx);
        ContinueWatchingItem {
            infohash: r.infohash,
            torrent_name: r.torrent_name,
            tmdb_id: r.tmdb_id,
            tmdb_verified: r.tmdb_verified,
            kind: r.kind.as_deref().and_then(MediaKind::from_wire),
            file_idx: r.file_idx,
            file_path,
            position_seconds: r.position_seconds,
            duration_seconds: r.duration_seconds,
            last_watched_at: r.last_watched_at,
            completed: r.completed,
            collection_id: r.collection_id,
            next_up: r.next_up,
            season: r.season,
            episode: r.episode,
            episode_name,
            poster_path,
            grabbable: r.grabbable,
        }
    })
    .await;
    Ok(Json(out))
}

/// Turn an owned shelf row whose file is no longer servable (GC'd, or
/// lost by the engine) into a grabbable tile in place. Returns `false`
/// when conversion isn't possible — the client didn't opt into grabbable
/// tiles, or the row has no `(collection, season, episode)` identity to
/// grab by (movies, unparsed files) — in which case the caller drops the
/// row.
fn revive_dead_row(
    r: &mut iris_db::playback::ContinueWatchingRow,
    include_grabbable: bool,
) -> bool {
    if !include_grabbable || r.collection_id.is_none() || r.season.is_none() || r.episode.is_none()
    {
        return false;
    }
    r.infohash = String::new();
    r.file_idx = 0;
    r.position_seconds = 0.0;
    r.duration_seconds = None;
    r.completed = false;
    r.audio_track_idx = None;
    r.subtitle_track_idx = None;
    r.next_up = true;
    r.grabbable = true;
    true
}

/// Watch-order gate for a next-up candidate. `prev` is the episode the user
/// just finished, `next` the candidate; both are `(season, episode)`.
/// Within a season only the immediate successor qualifies. Hopping to the
/// next season's opener additionally requires `prev` to be that season's
/// finale (`prev_season_finale`, from TMDB) — without confirmation the
/// candidate is dropped: a missing tile costs one manual navigation, a
/// wrong tile skips the user past unwatched episodes.
fn next_up_follows_watch_order(
    prev: (i64, i64),
    next: (i64, i64),
    prev_season_finale: Option<i64>,
) -> bool {
    let (prev_s, prev_e) = prev;
    let (next_s, next_e) = next;
    if next_s == prev_s {
        return next_e == prev_e + 1;
    }
    next_s == prev_s + 1 && next_e == 1 && prev_season_finale.is_some_and(|finale| prev_e >= finale)
}

/// Synthesised "grab the next episode" tiles (opt-in): for series whose
/// watch frontier got no owned tile from the resume/next-up merge (next
/// episode missing from disk — never downloaded, or GC'd), ask TMDB what
/// comes next and offer it if it has aired. Runs after the merge so an
/// owned tile always wins its collection's slot.
async fn append_grabbable_next_up(
    state: &AppState,
    user_id: iris_core::ids::UserId,
    merged: &mut Vec<iris_db::playback::ContinueWatchingRow>,
    by_collection: &mut std::collections::HashMap<uuid::Uuid, usize>,
) -> ApiResult<()> {
    let frontiers = iris_db::playback::continue_watching_frontiers(state.db(), user_id, 24).await?;
    let mut seen = std::collections::HashSet::new();
    let fresh = frontiers
        .into_iter()
        .filter(|f| !by_collection.contains_key(&f.collection_id) && seen.insert(f.collection_id));
    let aired = crate::fanout::map_ordered(fresh, |f| async move {
        let next = next_aired_episode(state, f.tmdb_id, f.prev_season, f.prev_episode).await;
        next.map(|e| (f, e))
    })
    .await;
    for (f, (season, episode)) in aired.into_iter().flatten() {
        by_collection.insert(f.collection_id, merged.len());
        merged.push(iris_db::playback::ContinueWatchingRow {
            infohash: String::new(),
            torrent_name: f.display_title,
            tmdb_id: f.tmdb_id,
            tmdb_verified: f.tmdb_id.is_some(),
            file_idx: 0,
            position_seconds: 0.0,
            duration_seconds: None,
            last_watched_at: f.last_watched_at,
            completed: false,
            audio_track_idx: None,
            subtitle_track_idx: None,
            kind: Some("tv".to_string()),
            collection_id: Some(f.collection_id),
            next_up: true,
            season: Some(season),
            episode: Some(episode),
            grabbable: true,
        });
    }
    Ok(())
}

/// The next episode after `(prev_season, prev_episode)` in TMDB's listing,
/// provided it has AIRED: `(s, e+1)` while the season lists more episodes,
/// else `(s+1, 1)` when a next season exists. `None` when TMDB can't
/// confirm (no client/id, unknown season, unaired or undated episode, or
/// the series is simply over) — no tile beats a dead one.
async fn next_aired_episode(
    state: &AppState,
    tmdb_id: Option<i64>,
    prev_season: i64,
    prev_episode: i64,
) -> Option<(i64, i64)> {
    let tmdb = state.tmdb()?;
    let id = u64::try_from(tmdb_id?).ok()?;
    let season = u32::try_from(prev_season).ok()?;
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let has_aired = |e: &crate::tmdb::EpisodeMetadata| {
        e.air_date.as_deref().is_some_and(|d| d <= today.as_str())
    };
    let eps = tmdb.tv_season_episodes(id, season).await;
    let finale = eps.iter().map(|e| i64::from(e.episode)).max()?;
    if prev_episode < finale {
        let target = prev_episode + 1;
        let ep = eps.iter().find(|e| i64::from(e.episode) == target)?;
        return has_aired(ep).then_some((prev_season, target));
    }
    let next = tmdb.tv_season_episodes(id, season + 1).await;
    let opener = next.iter().find(|e| e.episode == 1)?;
    has_aired(opener).then_some((prev_season + 1, 1))
}

/// Highest episode number TMDB lists for `season`, or `None` when it can't
/// be determined (no TMDB client, no id, TMDB down / unknown season — the
/// cases `tv_season_episodes` collapses into an empty list).
async fn season_finale_episode(state: &AppState, tmdb_id: Option<i64>, season: i64) -> Option<i64> {
    let tmdb = state.tmdb()?;
    let id = u64::try_from(tmdb_id?).ok()?;
    let season = u32::try_from(season).ok()?;
    let episodes = tmdb.tv_season_episodes(id, season).await;
    episodes.iter().map(|e| i64::from(e.episode)).max()
}

#[derive(Debug, serde::Deserialize, ToSchema)]
pub(crate) struct DismissCwRequest {
    /// Series to hide (TV). When set, the whole collection is removed from
    /// Continue Watching until the user plays a newer episode.
    collection_id: Option<uuid::Uuid>,
    /// Movie / standalone fallback: the exact file whose progress row is
    /// deleted. Ignored when `collection_id` is present.
    infohash: Option<String>,
    file_idx: Option<i64>,
}

#[utoipa::path(
    post,
    path = "/api/me/continue-watching/dismiss",
    request_body = DismissCwRequest,
    responses((status = 204, description = "Removed from the caller's Continue Watching")),
    tag = "me",
)]
pub(crate) async fn dismiss_continue_watching(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<DismissCwRequest>,
) -> ApiResult<axum::http::StatusCode> {
    if let Some(cid) = body.collection_id {
        // TV series — hide the whole collection (survives frontier regen).
        // Guard on existence: a stale id (series GC'd between the shelf fetch
        // and this call) has nothing to hide, and inserting it would trip the
        // `cw_dismissed` → `collections` foreign key. No-op is the right answer.
        if iris_db::collections::get(state.db(), cid).await?.is_some() {
            iris_db::playback::dismiss_collection(state.db(), user.id, cid).await?;
        }
    } else if let (Some(infohash), Some(file_idx)) = (body.infohash, body.file_idx) {
        // Movie / standalone — just drop the one progress row.
        iris_db::playback::delete(
            state.db(),
            user.id,
            &infohash.to_ascii_lowercase(),
            file_idx,
        )
        .await?;
    } else {
        return Err(ApiError::BadRequest(
            "need collection_id or (infohash, file_idx)".into(),
        ));
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Deserialize, ToSchema)]
pub(crate) struct DismissGoneRequest {
    /// Whole ghost collection to hide from the caller's Library grid
    /// (the greyed-out "GONE" card). When set, `infohash` is ignored.
    collection_id: Option<uuid::Uuid>,
    /// One reclaimed release to hide from the caller's gone rows on the
    /// collection page (both the per-episode ghost rows and the raw
    /// release row).
    infohash: Option<String>,
}

/// Per-user, timestamped, never destructive: History is untouched and
/// newer activity makes the dismissal stale (the entry returns).
#[utoipa::path(
    post,
    path = "/api/me/gone/dismiss",
    request_body = DismissGoneRequest,
    responses(
        (status = 204, description = "Hidden from the caller's Gone surfaces"),
        (status = 400, description = "Neither collection_id nor infohash provided"),
        (status = 401, description = "Not authenticated"),
    ),
    tag = "me",
)]
pub(crate) async fn dismiss_gone(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<DismissGoneRequest>,
) -> ApiResult<axum::http::StatusCode> {
    if let Some(cid) = body.collection_id {
        // Existence guard: a stale id has nothing to hide, and inserting
        // it would trip the `ghost_dismissed` → `collections` foreign key.
        if iris_db::collections::get(state.db(), cid).await?.is_some() {
            iris_db::collections::dismiss_ghost(state.db(), user.id, cid).await?;
        }
    } else if let Some(infohash) = body.infohash {
        let infohash = infohash.to_ascii_lowercase();
        // Same guard for the `gone_release_dismissed` → `torrents` FK.
        if iris_db::torrents::find_by_infohash(state.db(), &infohash)
            .await?
            .is_some()
        {
            iris_db::torrents::dismiss_gone_release(state.db(), user.id, &infohash).await?;
        }
    } else {
        return Err(ApiError::BadRequest(
            "need collection_id or infohash".into(),
        ));
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// One row of the caller's full watch history (in-progress AND completed),
/// including items whose source torrent has since been deleted
/// (disk-reclaim GC, admin cleanup) — unlike [`ContinueWatchingItem`], an
/// entry never just vanishes; `deleted` flags it instead so the client can
/// show "no longer available" rather than a dead resume link.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct HistoryItem {
    pub(crate) infohash: String,
    pub(crate) torrent_name: String,
    pub(crate) tmdb_id: Option<i64>,
    pub(crate) tmdb_verified: bool,
    pub(crate) kind: Option<MediaKind>,
    pub(crate) file_idx: i64,
    pub(crate) file_path: Option<String>,
    pub(crate) position_seconds: f64,
    pub(crate) duration_seconds: Option<f64>,
    pub(crate) last_watched_at: chrono::DateTime<chrono::Utc>,
    pub(crate) completed: bool,
    pub(crate) deleted: bool,
    /// Parent collection — survives GC (collections are never dropped),
    /// so clients group history under the show/movie and can route to
    /// the collection page even when every torrent was reclaimed (the
    /// "ghost collection" resume path). Additive — old clients ignore it.
    #[serde(default)]
    pub(crate) collection_id: Option<uuid::Uuid>,
    /// The collection's clean display title — the readable label to
    /// render instead of the raw SCENE torrent name.
    #[serde(default)]
    pub(crate) collection_title: Option<String>,
    /// SCENE-derived episode coordinates of the exact file watched
    /// (`S01E03`). Survive GC; a manual per-torrent remove drops them.
    #[serde(default)]
    pub(crate) season: Option<i64>,
    #[serde(default)]
    pub(crate) episode: Option<i64>,
    /// Absolute episode number for fleuve anime (render "Episode N").
    #[serde(default)]
    pub(crate) absolute_episode: Option<i64>,
    /// Source-torrent provenance: with both set, clients can offer
    /// "Download again" on a deleted row — re-resolving the same
    /// release yields the same infohash, so the stored resume
    /// position applies untouched.
    #[serde(default)]
    pub(crate) source_provider: Option<String>,
    #[serde(default)]
    pub(crate) source_external_id: Option<String>,
    /// TMDB poster path once `tmdb_verified`. Additive.
    #[serde(default)]
    pub(crate) poster_path: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/me/history",
    operation_id = "list_my_history",
    params(PageQuery),
    responses(
        (status = 200, description = "Caller's full watch history, including deleted-source items", body = [HistoryItem]),
        (status = 401, description = "Not authenticated"),
    ),
    tag = "me",
)]
pub(crate) async fn history(
    State(state): State<AppState>,
    user: AuthUser,
    axum::extract::Query(page): axum::extract::Query<PageQuery>,
) -> ApiResult<Json<Vec<HistoryItem>>> {
    Ok(Json(history_items(&state, user.id, &page).await?))
}

/// One user's history page, rendered — the caller's own (`/me/history`) and
/// the admin drill-down (`/admin/users/{id}/history`) share it.
pub(crate) async fn history_items(
    state: &AppState,
    user_id: iris_core::ids::UserId,
    page: &PageQuery,
) -> ApiResult<Vec<HistoryItem>> {
    let rows =
        iris_db::playback::user_history(state.db(), user_id, page.limit(), page.offset()).await?;
    Ok(crate::fanout::map_ordered(rows, |r| async move {
        let poster_path =
            library::verified_poster(state, r.tmdb_id, r.tmdb_verified, r.kind.as_deref()).await;
        let file_path = state.engine().file_name(&r.infohash, r.file_idx);
        HistoryItem {
            infohash: r.infohash,
            torrent_name: r.torrent_name,
            tmdb_id: r.tmdb_id,
            tmdb_verified: r.tmdb_verified,
            kind: r.kind.as_deref().and_then(MediaKind::from_wire),
            file_idx: r.file_idx,
            file_path,
            position_seconds: r.position_seconds,
            duration_seconds: r.duration_seconds,
            last_watched_at: r.last_watched_at,
            completed: r.completed,
            deleted: r.deleted,
            collection_id: r.collection_id,
            collection_title: r.collection_title,
            season: r.season,
            episode: r.episode,
            absolute_episode: r.absolute_episode,
            source_provider: r.source_provider,
            source_external_id: r.source_external_id,
            poster_path,
        }
    })
    .await)
}

#[cfg(test)]
mod tests {
    use super::{next_up_follows_watch_order, revive_dead_row};

    fn dead_row(
        collection: bool,
        season: Option<i64>,
        episode: Option<i64>,
    ) -> iris_db::playback::ContinueWatchingRow {
        iris_db::playback::ContinueWatchingRow {
            infohash: "abc123".into(),
            torrent_name: "Show.S01E06.1080p".into(),
            tmdb_id: Some(1396),
            tmdb_verified: true,
            file_idx: 3,
            position_seconds: 812.0,
            duration_seconds: Some(2760.0),
            last_watched_at: chrono::Utc::now(),
            completed: false,
            audio_track_idx: Some(1),
            subtitle_track_idx: Some(0),
            kind: Some("tv".into()),
            collection_id: collection.then(uuid::Uuid::new_v4),
            next_up: false,
            season,
            episode,
            grabbable: false,
        }
    }

    #[test]
    fn dead_row_with_episode_identity_becomes_grabbable() {
        let mut r = dead_row(true, Some(1), Some(6));
        assert!(revive_dead_row(&mut r, true));
        assert!(r.grabbable);
        assert!(r.next_up);
        assert_eq!(r.infohash, "");
        assert_eq!(r.file_idx, 0);
        assert!(r.position_seconds.abs() < f64::EPSILON);
        // Identity the grab endpoint needs survives the conversion.
        assert!(r.collection_id.is_some());
        assert_eq!((r.season, r.episode), (Some(1), Some(6)));
    }

    #[test]
    fn dead_row_without_identity_or_opt_in_is_dropped() {
        // Legacy client that never opted into grabbable tiles.
        assert!(!revive_dead_row(
            &mut dead_row(true, Some(1), Some(6)),
            false
        ));
        // Movie / standalone: no collection to grab through.
        assert!(!revive_dead_row(&mut dead_row(false, None, None), true));
        // Unparsed file inside a collection: no (S, E) to grab.
        assert!(!revive_dead_row(&mut dead_row(true, None, None), true));
        assert!(!revive_dead_row(&mut dead_row(true, Some(1), None), true));
    }

    #[test]
    fn same_season_only_immediate_successor() {
        assert!(next_up_follows_watch_order((8, 7), (8, 8), None));
        assert!(!next_up_follows_watch_order((8, 7), (8, 9), None));
        assert!(!next_up_follows_watch_order((8, 7), (8, 7), None));
    }

    #[test]
    fn season_hop_requires_confirmed_finale() {
        // The Rick and Morty bug: S08E07 done, S09E01 freshly on disk while
        // S08E08–E10 aren't. TMDB says S8 runs to E10 → not the finale.
        assert!(!next_up_follows_watch_order((8, 7), (9, 1), Some(10)));
        assert!(next_up_follows_watch_order((8, 10), (9, 1), Some(10)));
        // Odd numbering (specials folded in, TMDB shorter than disk):
        // anything at-or-past the listed finale counts as "season done".
        assert!(next_up_follows_watch_order((8, 12), (9, 1), Some(10)));
    }

    #[test]
    fn season_hop_without_tmdb_confirmation_is_dropped() {
        assert!(!next_up_follows_watch_order((8, 10), (9, 1), None));
    }

    #[test]
    fn only_the_immediate_next_season_opener_qualifies() {
        assert!(!next_up_follows_watch_order((8, 10), (10, 1), Some(10)));
        assert!(!next_up_follows_watch_order((8, 10), (9, 2), Some(10)));
    }
}
