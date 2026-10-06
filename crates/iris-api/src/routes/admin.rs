use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use chrono::{Duration, Utc};
use iris_auth::new_invitation_token;
use iris_core::ids::InvitationId;
use iris_core::search::MediaKind;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::{AdminUser, Infohash, Path};
use crate::routes::me::{HistoryItem, history_items};
use crate::routes::{PageQuery, page_limit};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/invitations",
            get(list_invitations).post(create_invitation),
        )
        .route(
            "/invitations/{id}",
            axum::routing::delete(revoke_invitation),
        )
        .route("/gc", axum::routing::post(trigger_gc))
        .route("/storage", get(storage_stats))
        .route("/users", get(list_users))
        .route("/users/{id}", axum::routing::delete(delete_user))
        .route(
            "/users/{id}/password",
            axum::routing::post(reset_user_password),
        )
        .route(
            "/users/{id}/display-name",
            axum::routing::post(set_user_display_name),
        )
        .route("/remux", get(list_remux_jobs))
        .route("/remux/{key}", axum::routing::delete(wipe_remux_job))
        .route("/tmdb/diagnose/{infohash}", get(diagnose_tmdb))
        .route("/active-sessions", get(active_sessions))
        .route("/watch-history", get(watch_history))
        .route("/users/{id}/history", get(user_history))
        .route("/audit-log", get(audit_log))
        .merge(super::providers::admin_router())
}

/// One live "who's watching what" row for `GET /admin/active-sessions`.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ActiveSessionView {
    user_id: Uuid,
    display_name: String,
    infohash: String,
    file_idx: i64,
    torrent_name: Option<String>,
    /// On-disk path of the exact file being watched. For season packs this
    /// is the only way to tell WHICH episode — the torrent name is the whole
    /// pack. `None` when the torrent snapshot isn't live (evicted).
    file_path: Option<String>,
    /// COALESCE(collection, torrent) tmdb id — only trust it for posters
    /// when `tmdb_verified` (mirrors the watch shelves).
    tmdb_id: Option<i64>,
    tmdb_verified: bool,
    /// `"movie"` / `"tv"` collection hint for the TMDB poster lookup.
    kind: Option<MediaKind>,
    position_seconds: f64,
    duration_seconds: Option<f64>,
    /// `"playing"` / `"paused"` / `"buffering"` (additive: older readers
    /// treat an unknown word as playing).
    state: &'static str,
    /// `"web"` / `"tv"` when the client identified itself, else null.
    client: Option<&'static str>,
    /// Semver of that client (the `version` half of `X-Iris-Client`).
    client_version: Option<String>,
    /// A web client's browser and system (« Firefox · macOS »). Additive.
    #[serde(default)]
    browser: Option<String>,
    started_at: chrono::DateTime<Utc>,
    last_seen_at: chrono::DateTime<Utc>,
    /// TMDB poster path once `tmdb_verified`. Additive.
    #[serde(default)]
    poster_path: Option<String>,
    /// What plays, in words (additive): its title, episode and year.
    #[serde(flatten)]
    title: PlayTitle,
}

/// What a play is, in words, for the admin's rows (additive fields of
/// [`ActiveSessionView`] and [`WatchHistoryView`]): the collection's title,
/// the episode of the exact file, and — once the TMDB match is verified —
/// the episode's name and the year.
#[derive(Debug, Default, Serialize, ToSchema)]
pub(crate) struct PlayTitle {
    /// The parent collection: its page, and the person's other plays of it.
    #[serde(default)]
    collection_id: Option<Uuid>,
    /// The collection's clean display title (never the release name).
    #[serde(default)]
    collection_title: Option<String>,
    #[serde(default)]
    season: Option<i64>,
    #[serde(default)]
    episode: Option<i64>,
    /// Absolute episode number for fleuve anime (render "Episode N").
    #[serde(default)]
    absolute_episode: Option<i64>,
    /// TMDB's name of the episode, verified matches only.
    #[serde(default)]
    episode_title: Option<String>,
    /// The title's year: TMDB's once verified, else a film release's own.
    #[serde(default)]
    year: Option<u32>,
}

/// The episode coordinates and collection of one play, as both rows carry them.
struct PlayKey<'a> {
    torrent_name: &'a str,
    tmdb_id: Option<i64>,
    tmdb_verified: bool,
    kind: Option<&'a str>,
    collection_id: Option<Uuid>,
    collection_title: Option<String>,
    season: Option<i64>,
    episode: Option<i64>,
    absolute_episode: Option<i64>,
}

/// A play's words and its poster (`None` unless the match is verified).
async fn play_title(state: &AppState, k: PlayKey<'_>) -> (PlayTitle, Option<String>) {
    let facts = crate::routes::library::watch_facts(
        state,
        k.tmdb_id,
        k.tmdb_verified,
        k.kind,
        k.season,
        k.episode,
    )
    .await;
    // a film's release names its year; a series' names the year of a season
    let year = facts.year.or_else(|| {
        (k.kind == Some("movie"))
            .then(|| iris_media::filename::parse(k.torrent_name).and_then(|p| p.year))
            .flatten()
            .map(u32::from)
    });
    (
        PlayTitle {
            collection_id: k.collection_id,
            collection_title: k.collection_title,
            season: k.season,
            episode: k.episode,
            absolute_episode: k.absolute_episode,
            episode_title: facts.episode_title,
            year,
        },
        facts.poster_path,
    )
}

#[utoipa::path(
    get,
    path = "/api/admin/active-sessions",
    operation_id = "list_active_sessions",
    responses(
        (status = 200, description = "Live 'who's watching what' presence rows", body = [ActiveSessionView]),
        (status = 403, description = "Caller is not an admin"),
    ),
    tag = "admin",
)]
pub(crate) async fn active_sessions(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> ApiResult<Json<Vec<ActiveSessionView>>> {
    let sessions = state.presence().snapshot().await;
    let mut out = Vec::with_capacity(sessions.len());
    for s in sessions {
        // Tiny N (≤ household size): per-session lookups are fine and reuse
        // the exact poster precedence of the watch shelves.
        let display_name =
            iris_db::users::find_by_id(state.db(), iris_core::ids::UserId::from(s.user_id))
                .await?
                .map_or_else(|| "unknown".to_owned(), |u| u.display_name);
        let card = iris_db::playback::session_card(state.db(), &s.infohash, s.file_idx).await?;
        let (title, poster_path) = match &card {
            Some(c) => {
                play_title(
                    &state,
                    PlayKey {
                        torrent_name: &c.torrent_name,
                        tmdb_id: c.tmdb_id,
                        tmdb_verified: c.tmdb_verified,
                        kind: c.kind.as_deref(),
                        collection_id: c.collection_id,
                        collection_title: c.collection_title.clone(),
                        season: c.season,
                        episode: c.episode,
                        absolute_episode: c.absolute_episode,
                    },
                )
                .await
            }
            None => (PlayTitle::default(), None),
        };
        out.push(ActiveSessionView {
            user_id: s.user_id,
            display_name,
            file_path: state.engine().file_name(&s.infohash, s.file_idx),
            infohash: s.infohash,
            file_idx: s.file_idx,
            torrent_name: card.as_ref().map(|c| c.torrent_name.clone()),
            tmdb_id: card.as_ref().and_then(|c| c.tmdb_id),
            tmdb_verified: card.as_ref().is_some_and(|c| c.tmdb_verified),
            kind: card
                .as_ref()
                .and_then(|c| c.kind.as_deref())
                .and_then(MediaKind::from_wire),
            position_seconds: s.position_seconds,
            duration_seconds: s.duration_seconds,
            state: s.state.as_str(),
            client: s.client.map(crate::client_version::ClientKind::as_str),
            client_version: s.client_version,
            browser: s.browser,
            started_at: s.started_at,
            last_seen_at: s.last_seen_at,
            poster_path,
            title,
        });
    }
    Ok(Json(out))
}

/// One row for `GET /admin/watch-history` — the household's plays, newest
/// first, reclaimed releases included (`deleted`).
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct WatchHistoryView {
    user_id: Uuid,
    display_name: String,
    infohash: String,
    file_idx: i64,
    torrent_name: String,
    /// On-disk path of the watched file (episode within a season pack).
    file_path: Option<String>,
    tmdb_id: Option<i64>,
    tmdb_verified: bool,
    kind: Option<MediaKind>,
    position_seconds: f64,
    duration_seconds: Option<f64>,
    completed: bool,
    last_watched_at: chrono::DateTime<Utc>,
    /// TMDB poster path once `tmdb_verified`. Additive.
    #[serde(default)]
    poster_path: Option<String>,
    /// The release was reclaimed from disk (its play stays). Additive.
    #[serde(default)]
    deleted: bool,
    /// What was played, in words (additive): its title, episode and year.
    #[serde(flatten)]
    title: PlayTitle,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct WatchHistoryQuery {
    /// Max rows to return (clamped 1..=200, defaults to 50).
    limit: Option<i64>,
    /// Pagination offset (defaults to 0).
    offset: Option<i64>,
    /// Only this person's plays.
    user_id: Option<Uuid>,
    /// Only series (`tv`) or only films (`movie`).
    kind: Option<MediaKind>,
}

#[utoipa::path(
    get,
    path = "/api/admin/watch-history",
    operation_id = "list_watch_history",
    params(WatchHistoryQuery),
    responses(
        (status = 200, description = "The household's plays, newest first", body = [WatchHistoryView]),
        (status = 403, description = "Caller is not an admin"),
    ),
    tag = "admin",
)]
pub(crate) async fn watch_history(
    State(state): State<AppState>,
    _admin: AdminUser,
    axum::extract::Query(q): axum::extract::Query<WatchHistoryQuery>,
) -> ApiResult<Json<Vec<WatchHistoryView>>> {
    let filter = iris_db::playback::HistoryFilter {
        user: q.user_id.map(iris_core::ids::UserId::from),
        kind: q.kind.map(MediaKind::as_wire),
    };
    let rows = iris_db::playback::household_history(
        state.db(),
        filter,
        page_limit(q.limit),
        q.offset.unwrap_or(0).max(0),
    )
    .await?;
    let state = &state;
    Ok(Json(
        crate::fanout::map_ordered(rows, |h| async move {
            let r = h.row;
            let (title, poster_path) = play_title(
                state,
                PlayKey {
                    torrent_name: &r.torrent_name,
                    tmdb_id: r.tmdb_id,
                    tmdb_verified: r.tmdb_verified,
                    kind: r.kind.as_deref(),
                    collection_id: r.collection_id,
                    collection_title: r.collection_title,
                    season: r.season,
                    episode: r.episode,
                    absolute_episode: r.absolute_episode,
                },
            )
            .await;
            WatchHistoryView {
                user_id: h.user_id,
                display_name: h.display_name,
                file_path: state.engine().file_name(&r.infohash, r.file_idx),
                infohash: r.infohash,
                file_idx: r.file_idx,
                torrent_name: r.torrent_name,
                tmdb_id: r.tmdb_id,
                tmdb_verified: r.tmdb_verified,
                kind: r.kind.as_deref().and_then(MediaKind::from_wire),
                position_seconds: r.position_seconds,
                duration_seconds: r.duration_seconds,
                completed: r.completed,
                last_watched_at: r.last_watched_at,
                poster_path,
                deleted: r.deleted,
                title,
            }
        })
        .await,
    ))
}

/// One row of a single user's full watch history for the admin drill-down
/// (`GET /admin/users/{id}/history`) — same shape as `me::HistoryItem`
/// (in-progress AND completed, survives source-torrent deletion via
/// `deleted`), just reached through the admin-only route instead of the
/// caller's own session.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct UserHistoryView {
    infohash: String,
    torrent_name: String,
    file_path: Option<String>,
    tmdb_id: Option<i64>,
    tmdb_verified: bool,
    kind: Option<MediaKind>,
    file_idx: i64,
    position_seconds: f64,
    duration_seconds: Option<f64>,
    completed: bool,
    last_watched_at: chrono::DateTime<Utc>,
    deleted: bool,
    /// Same additive grouping/provenance fields as `me::HistoryItem` —
    /// the admin drill-down renders through the identical client list.
    #[serde(default)]
    collection_id: Option<Uuid>,
    #[serde(default)]
    collection_title: Option<String>,
    #[serde(default)]
    season: Option<i64>,
    #[serde(default)]
    episode: Option<i64>,
    #[serde(default)]
    absolute_episode: Option<i64>,
    #[serde(default)]
    source_provider: Option<String>,
    #[serde(default)]
    source_external_id: Option<String>,
    /// TMDB poster path once `tmdb_verified`. Additive.
    #[serde(default)]
    poster_path: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/admin/users/{id}/history",
    operation_id = "list_user_history",
    params(
        ("id" = Uuid, Path, description = "Target user id"),
        PageQuery,
    ),
    responses(
        (status = 200, description = "Full watch history for one user, including deleted-source items", body = [UserHistoryView]),
        (status = 403, description = "Caller is not an admin"),
    ),
    tag = "admin",
)]
pub(crate) async fn user_history(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
    axum::extract::Query(page): axum::extract::Query<PageQuery>,
) -> ApiResult<Json<Vec<UserHistoryView>>> {
    let items = history_items(&state, iris_core::ids::UserId::from(id), &page).await?;
    Ok(Json(items.into_iter().map(UserHistoryView::from).collect()))
}

impl From<HistoryItem> for UserHistoryView {
    fn from(h: HistoryItem) -> Self {
        Self {
            infohash: h.infohash,
            torrent_name: h.torrent_name,
            file_path: h.file_path,
            tmdb_id: h.tmdb_id,
            tmdb_verified: h.tmdb_verified,
            kind: h.kind,
            file_idx: h.file_idx,
            position_seconds: h.position_seconds,
            duration_seconds: h.duration_seconds,
            completed: h.completed,
            last_watched_at: h.last_watched_at,
            deleted: h.deleted,
            collection_id: h.collection_id,
            collection_title: h.collection_title,
            season: h.season,
            episode: h.episode,
            absolute_episode: h.absolute_episode,
            source_provider: h.source_provider,
            source_external_id: h.source_external_id,
            poster_path: h.poster_path,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct UserView {
    id: Uuid,
    email: String,
    display_name: String,
    is_admin: bool,
    created_at: chrono::DateTime<Utc>,
    /// Their latest play, `None` when they never played anything. Additive.
    #[serde(default)]
    last_played_at: Option<chrono::DateTime<Utc>>,
    /// How many files they played (each once, however often resumed). Additive.
    #[serde(default)]
    plays: i64,
}

#[utoipa::path(
    get,
    path = "/api/admin/users",
    operation_id = "list_users",
    responses(
        (status = 200, description = "All registered users", body = [UserView]),
        (status = 403, description = "Caller is not an admin"),
    ),
    tag = "admin",
)]
pub(crate) async fn list_users(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> ApiResult<Json<Vec<UserView>>> {
    let users = iris_db::users::list(state.db()).await?;
    let activity: std::collections::HashMap<Uuid, iris_db::playback::UserActivityRow> =
        iris_db::playback::activity_by_user(state.db())
            .await?
            .into_iter()
            .map(|a| (a.user_id, a))
            .collect();
    Ok(Json(
        users
            .into_iter()
            .map(|u| {
                let id: Uuid = u.id.into();
                let seen = activity.get(&id);
                UserView {
                    id,
                    email: u.email,
                    display_name: u.display_name,
                    is_admin: u.is_admin,
                    created_at: u.created_at,
                    last_played_at: seen.map(|a| a.last_played_at),
                    plays: seen.map_or(0, |a| a.plays),
                }
            })
            .collect(),
    ))
}

/// Remove an account. Personal data (sessions, watch progress, follows,
/// preferences) cascades away; the target's grabs are re-attributed to
/// the acting admin so the shared library keeps every release. The
/// self-delete guard doubles as the "last admin standing" guarantee —
/// the caller is an admin and can't remove themselves.
#[utoipa::path(
    delete,
    path = "/api/admin/users/{id}",
    operation_id = "delete_user",
    params(("id" = Uuid, Path, description = "Target user id")),
    responses(
        (status = 204, description = "Account deleted; sessions revoked, grabs re-attributed to the caller"),
        (status = 400, description = "Cannot delete your own account"),
        (status = 403, description = "Caller is not an admin"),
        (status = 404, description = "No such user"),
    ),
    tag = "admin",
)]
pub(crate) async fn delete_user(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    let user_id = iris_core::ids::UserId::from(id);
    if user_id == admin.0.id {
        return Err(ApiError::BadRequest(
            "cannot delete your own account".into(),
        ));
    }
    let Some(target) = iris_db::users::find_by_id(state.db(), user_id).await? else {
        return Err(ApiError::NotFound);
    };
    if !iris_db::users::delete(state.db(), user_id, admin.0.id).await? {
        return Err(ApiError::NotFound);
    }
    state.session_cuts().forget(user_id);
    super::audit(
        &state,
        admin.0.id,
        "user.delete",
        "user",
        Some(&id.to_string()),
        Some(&target.email),
    )
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct ResetPasswordRequest {
    new_password: String,
}

#[utoipa::path(
    post,
    path = "/api/admin/users/{id}/password",
    operation_id = "reset_user_password",
    params(("id" = Uuid, Path)),
    request_body = ResetPasswordRequest,
    responses(
        (status = 204, description = "Password reset; every session revoked and every passkey removed"),
        (status = 400, description = "New password too short (min 8 chars)"),
        (status = 403, description = "Caller is not an admin"),
        (status = 404, description = "No such user"),
    ),
    tag = "admin",
)]
pub(crate) async fn reset_user_password(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(body): Json<ResetPasswordRequest>,
) -> ApiResult<axum::http::StatusCode> {
    crate::passwords::check_policy(&body.new_password)?;
    let user_id = iris_core::ids::UserId::from(id);
    let Some(target) = iris_db::users::find_by_id(state.db(), user_id).await? else {
        return Err(ApiError::NotFound);
    };
    let hash = crate::passwords::hash(&body.new_password).await?;
    let passkeys = iris_db::users::set_password(state.db(), user_id, &hash)
        .await?
        .ok_or(ApiError::NotFound)?;
    state.session_cuts().forget(user_id);
    super::audit(
        &state,
        admin.0.id,
        "user.password_reset",
        "user",
        Some(&id.to_string()),
        Some(&target.email),
    )
    .await;
    if passkeys > 0 {
        super::audit(
            &state,
            admin.0.id,
            "user.passkeys_revoked",
            "user",
            Some(&id.to_string()),
            Some(&format!(
                "{}: {passkeys} removed by the reset",
                target.email
            )),
        )
        .await;
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct SetDisplayNameRequest {
    display_name: String,
}

/// Admin-set another user's public display name. Mirrors the
/// self-service `POST /api/me/display-name` validation (non-empty,
/// max 64 chars after trimming) so both entry points agree.
#[utoipa::path(
    post,
    path = "/api/admin/users/{id}/display-name",
    operation_id = "set_user_display_name",
    params(("id" = Uuid, Path)),
    request_body = SetDisplayNameRequest,
    responses(
        (status = 204, description = "Display name updated"),
        (status = 400, description = "Empty / too-long display name (max 64)"),
        (status = 403, description = "Caller is not an admin"),
        (status = 404, description = "No such user"),
    ),
    tag = "admin",
)]
pub(crate) async fn set_user_display_name(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(body): Json<SetDisplayNameRequest>,
) -> ApiResult<axum::http::StatusCode> {
    let trimmed = super::me::checked_display_name(&body.display_name)?;
    let user_id = iris_core::ids::UserId::from(id);
    let updated = iris_db::users::update_display_name(state.db(), user_id, trimmed).await?;
    if !updated {
        return Err(ApiError::NotFound);
    }
    super::audit(
        &state,
        admin.0.id,
        "user.display_name_update",
        "user",
        Some(&id.to_string()),
        Some(trimmed),
    )
    .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/admin/gc",
    operation_id = "trigger_gc",
    responses(
        (status = 200, description = "Garbage-collection report", body = iris_torrent::GcReport),
        (status = 403, description = "Caller is not an admin"),
        (status = 500, description = "GC run failed"),
    ),
    tag = "admin",
)]
pub(crate) async fn trigger_gc(
    State(state): State<AppState>,
    admin: AdminUser,
) -> ApiResult<Json<iris_torrent::GcReport>> {
    let report = state
        .gc()
        .run_once()
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("gc: {e}")))?;
    // Only the manually-triggered run is audited — the background scheduler
    // sweep has no admin actor to attribute it to.
    let freed = report
        .used_bytes_before
        .saturating_sub(report.used_bytes_after);
    super::audit(
        &state,
        admin.0.id,
        "gc.evict",
        "torrent",
        None,
        Some(&format!(
            "{} torrent(s) evicted, {freed} bytes freed",
            report.evicted.len()
        )),
    )
    .await;
    Ok(Json(report))
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct StorageStats {
    used_bytes: u64,
    max_storage_bytes: u64,
    threshold_bytes: u64,
    target_bytes: u64,
    threshold_pct: u8,
    target_pct: u8,
    torrent_count: i64,
    /// Lifetime total uploaded across every torrent ever ingested,
    /// including soft-deleted ones. Reconciled from librqbit's session
    /// counter every 30 s — see `iris_api::seed_stats`.
    total_uploaded_bytes: u64,
    /// Lifetime total downloaded over the same population — the honest
    /// global-ratio denominator (current disk usage understates it once
    /// the GC has evicted anything). Additive field.
    #[serde(default)]
    total_downloaded_bytes: u64,
}

#[utoipa::path(
    get,
    path = "/api/admin/storage",
    operation_id = "get_storage_stats",
    responses(
        (status = 200, description = "Disk usage / cleanup thresholds / seed totals", body = StorageStats),
        (status = 403, description = "Caller is not an admin"),
    ),
    tag = "admin",
)]
pub(crate) async fn storage_stats(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> ApiResult<Json<StorageStats>> {
    let cfg = &state.cfg().storage;
    let max = cfg.max_storage_bytes();
    let used = iris_torrent::gc::dir_size(&cfg.download_dir)
        .await
        .unwrap_or(0);
    let count = iris_db::torrents::count_active(state.db())
        .await
        .unwrap_or(0);
    let (total_uploaded_bytes, total_downloaded_bytes) =
        iris_db::torrents::lifetime_bytes(state.db())
            .await
            .unwrap_or((0, 0));
    Ok(Json(StorageStats {
        used_bytes: used,
        max_storage_bytes: max,
        threshold_bytes: max * u64::from(cfg.cleanup_threshold_pct) / 100,
        target_bytes: max * u64::from(cfg.cleanup_target_pct) / 100,
        threshold_pct: cfg.cleanup_threshold_pct,
        target_pct: cfg.cleanup_target_pct,
        torrent_count: count,
        total_uploaded_bytes,
        total_downloaded_bytes,
    }))
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct InvitationView {
    id: Uuid,
    /// `None` once the admin who made it is deleted.
    created_by: Option<Uuid>,
    created_at: chrono::DateTime<Utc>,
    expires_at: chrono::DateTime<Utc>,
    consumed_at: Option<chrono::DateTime<Utc>>,
    consumed_by: Option<Uuid>,
}

#[utoipa::path(
    get,
    path = "/api/admin/invitations",
    operation_id = "list_invitations",
    responses(
        (status = 200, description = "All invitation tokens (hashes never returned)", body = [InvitationView]),
        (status = 403, description = "Caller is not an admin"),
    ),
    tag = "admin",
)]
pub(crate) async fn list_invitations(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> ApiResult<Json<Vec<InvitationView>>> {
    let rows = iris_db::invitations::list(state.db()).await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| InvitationView {
                id: r.id,
                created_by: r.created_by,
                created_at: r.created_at,
                expires_at: r.expires_at,
                consumed_at: r.consumed_at,
                consumed_by: r.consumed_by,
            })
            .collect(),
    ))
}

#[derive(Debug, Deserialize, Default, ToSchema)]
pub(crate) struct CreateInvitationRequest {
    ttl_secs: Option<i64>,
}

/// Past this, `Duration::seconds` and the expiry addition would overflow
/// (and panic) long before any useful horizon.
const MAX_INVITATION_TTL_SECS: i64 = 366 * 24 * 3600;

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct CreatedInvitation {
    id: Uuid,
    token: String,
    expires_at: chrono::DateTime<Utc>,
}

#[utoipa::path(
    post,
    path = "/api/admin/invitations",
    operation_id = "create_invitation",
    request_body = CreateInvitationRequest,
    responses(
        (status = 200, description = "Created invitation with its one-time plaintext token", body = CreatedInvitation),
        (status = 400, description = "TTL too short (min 60 s) or too long (max 1 year)"),
        (status = 403, description = "Caller is not an admin"),
    ),
    tag = "admin",
)]
pub(crate) async fn create_invitation(
    State(state): State<AppState>,
    admin: AdminUser,
    Json(req): Json<CreateInvitationRequest>,
) -> ApiResult<Json<CreatedInvitation>> {
    let ttl = req.ttl_secs.unwrap_or(state.cfg().auth.invitation_ttl_secs);
    if ttl < 60 {
        return Err(ApiError::BadRequest("ttl too short".into()));
    }
    if ttl > MAX_INVITATION_TTL_SECS {
        return Err(ApiError::BadRequest("ttl too long (max 1 year)".into()));
    }
    let expires_at = Utc::now() + Duration::seconds(ttl);
    let token = new_invitation_token();
    let row = iris_db::invitations::create(
        state.db(),
        iris_db::invitations::NewInvitation {
            token_hash: token.hash,
            created_by: admin.0.id,
            expires_at,
        },
    )
    .await?;
    super::audit(
        &state,
        admin.0.id,
        "invitation.create",
        "invitation",
        Some(&row.id.to_string()),
        Some(&format!("expires {}", row.expires_at.to_rfc3339())),
    )
    .await;
    Ok(Json(CreatedInvitation {
        id: row.id,
        token: token.plaintext,
        expires_at: row.expires_at,
    }))
}

#[utoipa::path(
    delete,
    path = "/api/admin/invitations/{id}",
    operation_id = "revoke_invitation",
    params(("id" = Uuid, Path)),
    responses(
        (status = 204, description = "Invitation revoked"),
        (status = 403, description = "Caller is not an admin"),
        (status = 404, description = "No such invitation"),
    ),
    tag = "admin",
)]
pub(crate) async fn revoke_invitation(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    let ok = iris_db::invitations::revoke(state.db(), InvitationId::from(id)).await?;
    if ok {
        super::audit(
            &state,
            admin.0.id,
            "invitation.revoke",
            "invitation",
            Some(&id.to_string()),
            None,
        )
        .await;
        Ok(axum::http::StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

/// One entry in the remuxer cache inventory shown in `/admin`.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct RemuxJobView {
    /// `<infohash>_<file_idx>` — also the cache filename stem.
    key: String,
    infohash: Option<String>,
    file_idx: Option<usize>,
    torrent_name: Option<String>,
    /// True if an ffmpeg run for this key is currently in flight.
    in_flight: bool,
    /// Bytes occupied by the cached `.fmp4` (0 when not built yet).
    size_bytes: u64,
    /// Last-modified time of the cache file (epoch seconds).
    mtime: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/admin/remux",
    operation_id = "list_remux_jobs",
    responses(
        (status = 200, description = "Remuxer cache inventory", body = [RemuxJobView]),
        (status = 403, description = "Caller is not an admin"),
    ),
    tag = "admin",
)]
pub(crate) async fn list_remux_jobs(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> ApiResult<Json<Vec<RemuxJobView>>> {
    let jobs = state.remuxer().list_jobs().await;
    let mut out = Vec::with_capacity(jobs.len());
    for j in jobs {
        let mut split = j.key.rsplitn(2, '_');
        let idx_str = split.next();
        let infohash = split.next().map(str::to_ascii_lowercase);
        let idx = idx_str.and_then(|s| s.parse::<usize>().ok());
        let torrent_name = if let Some(ih) = infohash.as_deref() {
            iris_db::torrents::find_by_infohash(state.db(), ih)
                .await
                .ok()
                .flatten()
                .map(|r| r.name)
        } else {
            None
        };
        out.push(RemuxJobView {
            key: j.key,
            infohash,
            file_idx: idx,
            torrent_name,
            in_flight: j.in_flight,
            size_bytes: j.size_bytes,
            mtime: j.mtime,
        });
    }
    Ok(Json(out))
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct WipeRemuxResponse {
    /// Bytes freed by removing the cache file.
    freed_bytes: u64,
}

/// Per-torrent TMDB resolution dump — `GET /admin/tmdb/diagnose/{infohash}`.
///
/// Surfaces every input the resolver sees so we can tell *why* a given
/// torrent is stuck on a wrong `tmdb_id`: the raw torrent name, what the
/// SCENE parser extracted, the multi-search candidates TMDB returned,
/// and the strict SCENE match the trust gate accepts (`picked`, `null`
/// when none). Tells a parser misextraction apart from a title TMDB files
/// under another name.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct TmdbDiagnose {
    infohash: String,
    torrent_name: String,
    db_tmdb_id: Option<i64>,
    db_tmdb_verified: bool,
    db_collection_id: Option<Uuid>,
    db_collection_tmdb_id: Option<i64>,
    parsed: Option<TmdbDiagnoseParsed>,
    /// Cleaned name fed to `multi_search`. Empty when SCENE parsing
    /// failed (no title to look up).
    cleaned_query: String,
    suggestions: Vec<TmdbDiagnoseSuggestion>,
    picked: Option<TmdbDiagnoseSuggestion>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct TmdbDiagnoseParsed {
    title: String,
    year: Option<u16>,
    season: Option<u32>,
    episode: Option<u32>,
    is_tv: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct TmdbDiagnoseSuggestion {
    kind: String,
    tmdb_id: u64,
    title: String,
    year: Option<u32>,
    poster_path: Option<String>,
}

impl From<crate::tmdb::TmdbSuggestion> for TmdbDiagnoseSuggestion {
    fn from(s: crate::tmdb::TmdbSuggestion) -> Self {
        Self {
            kind: s.kind.as_wire().to_owned(),
            tmdb_id: s.tmdb_id,
            title: s.title,
            year: s.year,
            poster_path: s.poster_path,
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/admin/tmdb/diagnose/{infohash}",
    operation_id = "diagnose_tmdb",
    params(("infohash" = String, Path)),
    responses(
        (status = 200, description = "Full TMDB-resolution dump for one torrent", body = TmdbDiagnose),
        (status = 403, description = "Caller is not an admin"),
        (status = 404, description = "No such torrent"),
    ),
    tag = "admin",
)]
pub(crate) async fn diagnose_tmdb(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(infohash): Path<Infohash>,
) -> ApiResult<Json<TmdbDiagnose>> {
    let infohash = infohash.into_inner();
    let row = crate::routes::torrents::torrent_or_404(&state, &infohash).await?;

    let collection_tmdb_id = match row.collection_id {
        Some(cid) => iris_db::collections::get(state.db(), cid)
            .await?
            .and_then(|c| c.tmdb_id),
        None => None,
    };

    let parsed = iris_media::filename::parse(&row.name);
    let cleaned = parsed
        .as_ref()
        .map(|p| iris_media::filename::series_key(&p.title))
        .unwrap_or_default();

    let mut suggestions: Vec<TmdbDiagnoseSuggestion> = Vec::new();
    let mut picked: Option<TmdbDiagnoseSuggestion> = None;

    if let (Some(tmdb), Some(p)) = (state.tmdb(), parsed.as_ref())
        && cleaned.len() >= 2
    {
        let raw = tmdb.multi_search(&cleaned).await.unwrap_or_default();
        suggestions.extend(raw.into_iter().map(TmdbDiagnoseSuggestion::from));
        // The strict SCENE match (trust signal T2) the ingest path applies.
        picked = crate::tmdb_trust::strict_scene(
            state.db(),
            tmdb,
            &p.title,
            crate::tmdb_resolve::parsed_kind(p),
            p.year.map(u32::from),
        )
        .await
        .ok()
        .flatten()
        .map(TmdbDiagnoseSuggestion::from);
    }

    Ok(Json(TmdbDiagnose {
        infohash,
        torrent_name: row.name,
        db_tmdb_id: row.tmdb_id,
        db_tmdb_verified: row.tmdb_verified,
        db_collection_id: row.collection_id,
        db_collection_tmdb_id: collection_tmdb_id,
        parsed: parsed.map(|p| TmdbDiagnoseParsed {
            title: p.title.clone(),
            year: p.year,
            season: p.season,
            episode: p.episode,
            is_tv: p.is_tv(),
        }),
        cleaned_query: cleaned,
        suggestions,
        picked,
    }))
}

#[utoipa::path(
    delete,
    path = "/api/admin/remux/{key}",
    operation_id = "wipe_remux_job",
    params(("key" = String, Path)),
    responses(
        (status = 200, description = "Cache file removed; bytes freed", body = WipeRemuxResponse),
        (status = 400, description = "Invalid remux job key"),
        (status = 403, description = "Caller is not an admin"),
        (status = 500, description = "Remux wipe failed"),
    ),
    tag = "admin",
)]
pub(crate) async fn wipe_remux_job(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(key): Path<String>,
) -> ApiResult<Json<WipeRemuxResponse>> {
    // Defensive: only accept `<hex>_<digits>` to keep this path-traversal-free.
    if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || key.is_empty() {
        return Err(ApiError::BadRequest("invalid remux job key".into()));
    }
    let freed = state
        .remuxer()
        .wipe(&key)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("remux wipe: {e}")))?;
    super::audit(
        &state,
        admin.0.id,
        "remux.wipe",
        "remux_job",
        Some(&key),
        Some(&format!("{freed} bytes freed")),
    )
    .await;
    Ok(Json(WipeRemuxResponse { freed_bytes: freed }))
}

/// One row of `GET /admin/audit-log` — a persisted, queryable record of
/// sensitive actions (deletions, password resets, admin-triggered GC),
/// replacing the previous ephemeral `tracing::` logs.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct AuditLogView {
    id: i64,
    actor_id: Uuid,
    actor_display_name: String,
    action: String,
    resource_type: String,
    resource_id: Option<String>,
    details: Option<String>,
    created_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct AuditLogQuery {
    /// Max rows to return (clamped 1..=200, defaults to 50).
    limit: Option<i64>,
    /// Pagination offset (defaults to 0).
    offset: Option<i64>,
    /// One action (`user.delete`) or a family of them (`user`: every `user.*`).
    action: Option<String>,
    /// Only what this person did.
    actor_id: Option<Uuid>,
}

#[utoipa::path(
    get,
    path = "/api/admin/audit-log",
    operation_id = "list_audit_log",
    params(AuditLogQuery),
    responses(
        (status = 200, description = "Audited actions, newest first", body = [AuditLogView]),
        (status = 403, description = "Caller is not an admin"),
    ),
    tag = "admin",
)]
pub(crate) async fn audit_log(
    State(state): State<AppState>,
    _admin: AdminUser,
    axum::extract::Query(q): axum::extract::Query<AuditLogQuery>,
) -> ApiResult<Json<Vec<AuditLogView>>> {
    let filter = iris_db::audit::AuditFilter {
        action: q.action.as_deref().filter(|a| !a.is_empty()),
        actor: q.actor_id.map(iris_core::ids::UserId::from),
    };
    let rows = iris_db::audit::list_filtered(
        state.db(),
        filter,
        page_limit(q.limit),
        q.offset.unwrap_or(0).max(0),
    )
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| AuditLogView {
                id: r.id,
                actor_id: r.actor_id,
                actor_display_name: r.actor_display_name,
                action: r.action,
                resource_type: r.resource_type,
                resource_id: r.resource_id,
                details: r.details,
                created_at: r.created_at,
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use iris_core::ids::UserId;
    use iris_db::collections::Kind;
    use iris_db::test_support::{make_named_user, migrated_pool};
    use iris_providers::ProviderRegistry;
    use serde_json::Value;
    use tower::ServiceExt;
    use uuid::Uuid;

    use crate::state::AppState;

    async fn get(app: &axum::Router, path: &str, token: &str) -> (StatusCode, Value) {
        let req = Request::builder()
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn torrent(
        pool: &sqlx::SqlitePool,
        owner: UserId,
        name: &str,
    ) -> iris_db::torrents::TorrentRow {
        iris_db::torrents::upsert(
            pool,
            iris_db::torrents::NewTorrent {
                infohash: Uuid::new_v4().simple().to_string(),
                name: name.to_owned(),
                total_size_bytes: 1_000,
                source_provider: None,
                source_external_id: None,
                tracker_tmdb_id: None,
                added_by: owner,
            },
        )
        .await
        .unwrap()
    }

    async fn play(pool: &sqlx::SqlitePool, user: UserId, infohash: &str, completed: bool) {
        iris_db::playback::upsert(
            pool,
            iris_db::playback::UpsertProgress {
                user_id: user,
                infohash: infohash.to_owned(),
                file_idx: 0,
                position_seconds: 600.0,
                duration_seconds: Some(3_000.0),
                audio_track_idx: None,
                subtitle_track_idx: None,
                completed,
            },
        )
        .await
        .unwrap();
    }

    /// Ana watched an episode of Severance, Bo finished Dune (since reclaimed
    /// from disk); the admin watched nothing.
    async fn household() -> (AppState, axum::Router, String, String, UserId, UserId) {
        let pool = migrated_pool().await;
        let admin = make_named_user(&pool, "Admin").await;
        let ana = make_named_user(&pool, "Ana").await;
        let bo = make_named_user(&pool, "Bo").await;
        let show = iris_db::collections::create_standalone(&pool, "Severance", Kind::Tv)
            .await
            .unwrap();
        let film = iris_db::collections::create_standalone(&pool, "Dune", Kind::Movie)
            .await
            .unwrap();
        let ep = torrent(&pool, ana, "Severance.S02E04.1080p.WEB.H264-GROUP").await;
        iris_db::torrents::set_collection(&pool, &ep.infohash, Some(show.id))
            .await
            .unwrap();
        iris_db::episode_files::upsert(
            &pool,
            iris_db::episode_files::UpsertEpisodeFile {
                collection_id: show.id,
                season: 2,
                episode: 4,
                infohash: ep.infohash.clone(),
                file_idx: 0,
                derived_from: iris_db::episode_files::DerivedFrom::SceneParse,
                absolute_episode: None,
            },
        )
        .await
        .unwrap();
        let movie = torrent(&pool, bo, "Dune.2021.2160p.UHD.BluRay.x265-GROUP").await;
        iris_db::torrents::set_collection(&pool, &movie.infohash, Some(film.id))
            .await
            .unwrap();
        play(&pool, bo, &movie.infohash, true).await;
        play(&pool, ana, &ep.infohash, false).await;
        iris_db::torrents::soft_delete(&pool, iris_core::ids::TorrentId::from(movie.id))
            .await
            .unwrap();
        let state = AppState::for_tests(pool, ProviderRegistry::from_entries(&[]).unwrap()).await;
        let admin_token = state.jwt().issue_access(admin, true).unwrap();
        let member_token = state.jwt().issue_access(ana, false).unwrap();
        let app = crate::app::build_router(state.clone());
        (state, app, admin_token, member_token, ana, bo)
    }

    #[tokio::test]
    async fn creating_and_revoking_an_invitation_is_audited() {
        let (state, app, admin, _, _, _) = household().await;
        let created = crate::routes::auth::tests::call(
            &app,
            "POST",
            "/api/admin/invitations",
            Some(&admin),
            None,
            Some(serde_json::json!({})),
        )
        .await;
        assert_eq!(created.status, StatusCode::OK);
        let id = created.json["id"].as_str().unwrap().to_owned();
        let revoked = crate::routes::auth::tests::call(
            &app,
            "DELETE",
            &format!("/api/admin/invitations/{id}"),
            Some(&admin),
            None,
            None,
        )
        .await;
        assert_eq!(revoked.status, StatusCode::NO_CONTENT);
        let actions: Vec<String> = iris_db::audit::list(state.db(), 50, 0)
            .await
            .unwrap()
            .into_iter()
            .filter(|r| r.resource_id.as_deref() == Some(id.as_str()))
            .map(|r| r.action)
            .collect();
        assert_eq!(actions.len(), 2, "{actions:?}");
        assert!(actions.iter().any(|a| a == "invitation.create"));
        assert!(actions.iter().any(|a| a == "invitation.revoke"));
    }

    #[tokio::test]
    async fn watch_history_names_each_play_and_filters_by_person_and_kind() {
        let (_state, app, admin, member, ana, bo) = household().await;
        let (status, _) = get(&app, "/api/admin/watch-history", &member).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (status, all) = get(&app, "/api/admin/watch-history", &admin).await;
        assert_eq!(status, StatusCode::OK);
        let all = all.as_array().unwrap();
        assert_eq!(all.len(), 2, "a reclaimed release keeps its play");
        let sev = all.iter().find(|r| r["display_name"] == "Ana").unwrap();
        assert_eq!(sev["collection_title"], "Severance");
        assert_eq!(
            (sev["season"].as_i64(), sev["episode"].as_i64()),
            (Some(2), Some(4))
        );
        assert_eq!(
            sev["year"],
            Value::Null,
            "a series' release names no title year"
        );
        assert_eq!(sev["poster_path"], Value::Null, "unverified: no poster");
        assert_eq!(sev["deleted"], false);
        let dune = all.iter().find(|r| r["display_name"] == "Bo").unwrap();
        assert_eq!(dune["collection_title"], "Dune");
        assert_eq!(dune["year"], 2021);
        assert_eq!(dune["deleted"], true);
        assert_eq!(dune["completed"], true);

        let (_, page) = get(&app, "/api/admin/watch-history?limit=1&offset=1", &admin).await;
        assert_eq!(page.as_array().unwrap().len(), 1);
        assert_eq!(
            page[0]["display_name"], "Bo",
            "newest first: Bo's play is the older"
        );

        let (_, anas) = get(
            &app,
            &format!("/api/admin/watch-history?user_id={}", Uuid::from(ana)),
            &admin,
        )
        .await;
        assert_eq!(anas.as_array().unwrap().len(), 1);
        assert_eq!(anas[0]["collection_title"], "Severance");
        let (_, films) = get(&app, "/api/admin/watch-history?kind=movie", &admin).await;
        assert_eq!(films.as_array().unwrap().len(), 1);
        assert_eq!(films[0]["user_id"], Uuid::from(bo).to_string());
        let (status, _) = get(&app, "/api/admin/watch-history?kind=opera", &admin).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn users_say_when_each_person_last_played() {
        let (_state, app, admin, _, _, _) = household().await;
        let (status, users) = get(&app, "/api/admin/users", &admin).await;
        assert_eq!(status, StatusCode::OK);
        let by = |name: &str| {
            users
                .as_array()
                .unwrap()
                .iter()
                .find(|u| u["display_name"] == name)
                .unwrap()
                .clone()
        };
        assert_eq!(by("Admin")["last_played_at"], Value::Null);
        assert_eq!(by("Admin")["plays"], 0);
        assert!(by("Ana")["last_played_at"].is_string());
        assert_eq!(by("Bo")["plays"], 1);
    }

    #[tokio::test]
    async fn the_audit_log_filters_by_action_family_and_actor() {
        let (state, app, admin, _, ana, bo) = household().await;
        for (actor, action) in [
            (ana, "torrent.delete"),
            (bo, "user.password_change"),
            (bo, "torrent.delete"),
        ] {
            crate::routes::audit(&state, actor, action, "x", None, None).await;
        }
        let (_, torrents) = get(&app, "/api/admin/audit-log?action=torrent", &admin).await;
        assert_eq!(torrents.as_array().unwrap().len(), 2);
        let (_, bos) = get(
            &app,
            &format!(
                "/api/admin/audit-log?action=torrent.delete&actor_id={}",
                Uuid::from(bo)
            ),
            &admin,
        )
        .await;
        assert_eq!(bos.as_array().unwrap().len(), 1);
        assert_eq!(bos[0]["actor_display_name"], "Bo");
        let (_, everything) = get(&app, "/api/admin/audit-log?action=&limit=2", &admin).await;
        assert_eq!(
            everything.as_array().unwrap().len(),
            2,
            "an empty action is no filter"
        );
    }

    #[tokio::test]
    async fn now_watching_names_the_title_and_episode() {
        let (state, app, admin, _, ana, _) = household().await;
        let (_, plays) = get(&app, "/api/admin/watch-history?kind=tv", &admin).await;
        let infohash = plays[0]["infohash"].as_str().unwrap().to_owned();
        state
            .presence()
            .touch(crate::presence::Heartbeat {
                user_id: ana.into(),
                infohash,
                file_idx: 0,
                position_seconds: 120.0,
                duration_seconds: Some(3_000.0),
                state: crate::presence::PlaybackState::Playing,
                client: None,
                client_version: None,
                browser: None,
            })
            .await;
        let (status, now) = get(&app, "/api/admin/active-sessions", &admin).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(now.as_array().unwrap().len(), 1);
        assert_eq!(now[0]["display_name"], "Ana");
        assert_eq!(now[0]["collection_title"], "Severance");
        assert_eq!(
            (now[0]["season"].as_i64(), now[0]["episode"].as_i64()),
            (Some(2), Some(4))
        );
        assert_eq!(now[0]["state"], "playing");
    }
}
