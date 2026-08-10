use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::routing::get;
use chrono::{Duration, Utc};
use iris_auth::new_invitation_token;
use iris_core::ids::InvitationId;
use iris_core::search::MediaKind;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::AdminUser;
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
}

/// Resolve the on-disk file name for `(infohash, file_idx)` from the live
/// torrent snapshot. For season packs this disambiguates the episode being
/// watched — the torrent name alone can't. Mirrors the lookup
/// `me::continue_watching` does for the home shelf.
fn resolve_file_path(state: &AppState, infohash: &str, file_idx: i64) -> Option<String> {
    let idx = usize::try_from(file_idx).ok()?;
    state
        .engine()
        .get_by_infohash(infohash)?
        .files
        .into_iter()
        .find(|f| f.index == idx)
        .map(|f| f.path)
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
    /// `"playing"` / `"paused"`.
    state: &'static str,
    /// `"web"` / `"tv"` when the client identified itself, else null.
    client: Option<&'static str>,
    /// Semver of that client (the `version` half of `X-Iris-Client`).
    client_version: Option<String>,
    started_at: chrono::DateTime<Utc>,
    last_seen_at: chrono::DateTime<Utc>,
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
        let card = iris_db::playback::session_card(state.db(), &s.infohash).await?;
        out.push(ActiveSessionView {
            user_id: s.user_id,
            display_name,
            file_path: resolve_file_path(&state, &s.infohash, s.file_idx),
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
            started_at: s.started_at,
            last_seen_at: s.last_seen_at,
        });
    }
    Ok(Json(out))
}

/// One row for `GET /admin/watch-history` — recent playback across all users.
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
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct WatchHistoryQuery {
    /// Max rows to return (clamped 1..=200, defaults to 50).
    limit: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/admin/watch-history",
    operation_id = "list_watch_history",
    params(WatchHistoryQuery),
    responses(
        (status = 200, description = "Recent playback across all users", body = [WatchHistoryView]),
        (status = 403, description = "Caller is not an admin"),
    ),
    tag = "admin",
)]
pub(crate) async fn watch_history(
    State(state): State<AppState>,
    _admin: AdminUser,
    axum::extract::Query(q): axum::extract::Query<WatchHistoryQuery>,
) -> ApiResult<Json<Vec<WatchHistoryView>>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let rows = iris_db::playback::recent_activity(state.db(), limit).await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| WatchHistoryView {
                user_id: r.user_id,
                display_name: r.display_name,
                file_path: resolve_file_path(&state, &r.infohash, r.file_idx),
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
            })
            .collect(),
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
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct UserHistoryQuery {
    /// Max rows to return (clamped 1..=200, defaults to 50).
    limit: Option<i64>,
    /// Pagination offset (defaults to 0).
    offset: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/admin/users/{id}/history",
    operation_id = "list_user_history",
    params(
        ("id" = Uuid, Path, description = "Target user id"),
        UserHistoryQuery,
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
    axum::extract::Query(q): axum::extract::Query<UserHistoryQuery>,
) -> ApiResult<Json<Vec<UserHistoryView>>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let offset = q.offset.unwrap_or(0).max(0);
    let rows = iris_db::playback::user_history(
        state.db(),
        iris_core::ids::UserId::from(id),
        limit,
        offset,
    )
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| UserHistoryView {
                file_path: resolve_file_path(&state, &r.infohash, r.file_idx),
                infohash: r.infohash,
                torrent_name: r.torrent_name,
                tmdb_id: r.tmdb_id,
                tmdb_verified: r.tmdb_verified,
                kind: r.kind.as_deref().and_then(MediaKind::from_wire),
                file_idx: r.file_idx,
                position_seconds: r.position_seconds,
                duration_seconds: r.duration_seconds,
                completed: r.completed,
                last_watched_at: r.last_watched_at,
                deleted: r.deleted,
                collection_id: r.collection_id,
                collection_title: r.collection_title,
                season: r.season,
                episode: r.episode,
                absolute_episode: r.absolute_episode,
                source_provider: r.source_provider,
                source_external_id: r.source_external_id,
            })
            .collect(),
    ))
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct UserView {
    id: Uuid,
    email: String,
    display_name: String,
    is_admin: bool,
    created_at: chrono::DateTime<Utc>,
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
    Ok(Json(
        users
            .into_iter()
            .map(|u| UserView {
                id: u.id.into(),
                email: u.email,
                display_name: u.display_name,
                is_admin: u.is_admin,
                created_at: u.created_at,
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
    if let Err(e) = iris_db::audit::record(
        state.db(),
        admin.0.id,
        "user.delete",
        "user",
        Some(&id.to_string()),
        Some(&target.email),
    )
    .await
    {
        tracing::warn!(error = %e, user_id = %id, "audit log write failed");
    }
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
        (status = 204, description = "Password reset; all refresh tokens revoked"),
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
    if body.new_password.len() < 8 {
        return Err(ApiError::BadRequest(
            "new password too short (min 8 chars)".into(),
        ));
    }
    let user_id = iris_core::ids::UserId::from(id);
    let Some(target) = iris_db::users::find_by_id(state.db(), user_id).await? else {
        return Err(ApiError::NotFound);
    };
    let hash = iris_auth::hash_password(&body.new_password)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("hash: {e}")))?;
    iris_db::users::update_password_hash(state.db(), user_id, &hash).await?;
    iris_db::refresh_tokens::revoke_all_for_user(state.db(), user_id).await?;
    if let Err(e) = iris_db::audit::record(
        state.db(),
        admin.0.id,
        "user.password_reset",
        "user",
        Some(&id.to_string()),
        Some(&target.email),
    )
    .await
    {
        tracing::warn!(error = %e, user_id = %id, "audit log write failed");
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
    let trimmed = body.display_name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("display name cannot be empty".into()));
    }
    if trimmed.len() > 64 {
        return Err(ApiError::BadRequest(
            "display name too long (max 64)".into(),
        ));
    }
    let user_id = iris_core::ids::UserId::from(id);
    let updated = iris_db::users::update_display_name(state.db(), user_id, trimmed).await?;
    if !updated {
        return Err(ApiError::NotFound);
    }
    if let Err(e) = iris_db::audit::record(
        state.db(),
        admin.0.id,
        "user.display_name_update",
        "user",
        Some(&id.to_string()),
        Some(trimmed),
    )
    .await
    {
        tracing::warn!(error = %e, user_id = %id, "audit log write failed");
    }
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
    if let Err(e) = iris_db::audit::record(
        state.db(),
        admin.0.id,
        "gc.evict",
        "torrent",
        None,
        Some(&format!(
            "{} torrent(s) evicted, {freed} bytes freed",
            report.evicted.len()
        )),
    )
    .await
    {
        tracing::warn!(error = %e, "audit log write failed");
    }
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
    let max = cfg.max_storage_gb.saturating_mul(1_073_741_824);
    let used = dir_size_async(&cfg.download_dir).await.unwrap_or(0);
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM torrents WHERE deleted_at IS NULL")
        .fetch_one(state.db())
        .await
        .unwrap_or((0,));
    let total_uploaded_bytes = iris_db::torrents::total_uploaded_bytes(state.db())
        .await
        .unwrap_or(0);
    Ok(Json(StorageStats {
        used_bytes: used,
        max_storage_bytes: max,
        threshold_bytes: max * u64::from(cfg.cleanup_threshold_pct) / 100,
        target_bytes: max * u64::from(cfg.cleanup_target_pct) / 100,
        threshold_pct: cfg.cleanup_threshold_pct,
        target_pct: cfg.cleanup_target_pct,
        torrent_count: count.0,
        total_uploaded_bytes,
    }))
}

async fn dir_size_async(path: &std::path::Path) -> std::io::Result<u64> {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(p) = stack.pop() {
        let mut read = match tokio::fs::read_dir(&p).await {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        while let Some(entry) = read.next_entry().await? {
            let Ok(m) = entry.metadata().await else {
                continue;
            };
            if m.is_dir() {
                stack.push(entry.path());
            } else {
                total += m.len();
            }
        }
    }
    Ok(total)
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct InvitationView {
    id: Uuid,
    created_by: Uuid,
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
        (status = 400, description = "TTL too short (min 60s)"),
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
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    let ok = iris_db::invitations::revoke(state.db(), InvitationId::from(id)).await?;
    if ok {
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
/// and what `pick_best` would settle on with the current rules. Useful
/// when a library card shows the wrong poster and we need to know
/// whether the bug is upstream of `pick_best` (parser misextracted the
/// title, TMDB has the wrong show on file) or downstream (our scoring
/// picks a worse candidate).
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
    Path(infohash): Path<String>,
) -> ApiResult<Json<TmdbDiagnose>> {
    let infohash = infohash.to_ascii_lowercase();
    let row = iris_db::torrents::find_by_infohash(state.db(), &infohash)
        .await?
        .ok_or(ApiError::NotFound)?;

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
        let raw = tmdb.multi_search(&cleaned).await;
        for s in &raw {
            suggestions.push(TmdbDiagnoseSuggestion {
                kind: format!("{:?}", s.kind).to_ascii_lowercase(),
                tmdb_id: s.tmdb_id,
                title: s.title.clone(),
                year: s.year,
                poster_path: s.poster_path.clone(),
            });
        }
        // Re-run resolution end-to-end so the dump reflects what the
        // backfill / ingestion path would actually pick today.
        let kind_hint = if p.is_tv() {
            Some(crate::tmdb::TmdbKind::Tv)
        } else {
            Some(crate::tmdb::TmdbKind::Movie)
        };
        if let Some(r) = crate::tmdb_resolve::resolve_cleaned(
            state.db(),
            tmdb,
            &cleaned,
            kind_hint,
            p.year.map(u32::from),
        )
        .await
        {
            picked = Some(TmdbDiagnoseSuggestion {
                kind: format!("{:?}", r.kind).to_ascii_lowercase(),
                tmdb_id: r.tmdb_id,
                title: r.title,
                year: r.year,
                poster_path: r.poster_path,
            });
        }
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
    if let Err(e) = iris_db::audit::record(
        state.db(),
        admin.0.id,
        "remux.wipe",
        "remux_job",
        Some(&key),
        Some(&format!("{freed} bytes freed")),
    )
    .await
    {
        tracing::warn!(error = %e, key = %key, "audit log write failed");
    }
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
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let offset = q.offset.unwrap_or(0).max(0);
    let rows = iris_db::audit::list(state.db(), limit, offset).await?;
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
