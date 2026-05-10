use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::routing::get;
use chrono::{Duration, Utc};
use iris_auth::new_invitation_token;
use iris_core::ids::InvitationId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::AdminUser;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/invitations", get(list_invitations).post(create_invitation))
        .route("/invitations/{id}", axum::routing::delete(revoke_invitation))
        .route("/gc", axum::routing::post(trigger_gc))
        .route("/storage", get(storage_stats))
        .route("/users", get(list_users))
        .route(
            "/users/{id}/password",
            axum::routing::post(reset_user_password),
        )
        .route("/remux", get(list_remux_jobs))
        .route("/remux/{key}", axum::routing::delete(wipe_remux_job))
}

#[derive(Debug, Serialize)]
struct UserView {
    id: Uuid,
    email: String,
    display_name: String,
    is_admin: bool,
    created_at: chrono::DateTime<Utc>,
}

async fn list_users(
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

#[derive(Debug, Deserialize)]
struct ResetPasswordRequest {
    new_password: String,
}

async fn reset_user_password(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(body): Json<ResetPasswordRequest>,
) -> ApiResult<axum::http::StatusCode> {
    if body.new_password.len() < 8 {
        return Err(ApiError::BadRequest(
            "new password too short (min 8 chars)".into(),
        ));
    }
    let user_id = iris_core::ids::UserId::from(id);
    let exists = iris_db::users::find_by_id(state.db(), user_id).await?;
    if exists.is_none() {
        return Err(ApiError::NotFound);
    }
    let hash = iris_auth::hash_password(&body.new_password)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("hash: {e}")))?;
    iris_db::users::update_password_hash(state.db(), user_id, &hash).await?;
    iris_db::refresh_tokens::revoke_all_for_user(state.db(), user_id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn trigger_gc(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> ApiResult<Json<iris_torrent::GcReport>> {
    let report = state
        .gc()
        .run_once()
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("gc: {e}")))?;
    Ok(Json(report))
}

#[derive(Debug, Serialize)]
struct StorageStats {
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

async fn storage_stats(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> ApiResult<Json<StorageStats>> {
    let cfg = &state.cfg().storage;
    let max = cfg.max_storage_gb.saturating_mul(1_073_741_824);
    let used = dir_size_async(&cfg.download_dir).await.unwrap_or(0);
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM torrents WHERE deleted_at IS NULL")
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

#[derive(Debug, Serialize)]
struct InvitationView {
    id: Uuid,
    created_by: Uuid,
    created_at: chrono::DateTime<Utc>,
    expires_at: chrono::DateTime<Utc>,
    consumed_at: Option<chrono::DateTime<Utc>>,
    consumed_by: Option<Uuid>,
}

async fn list_invitations(
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

#[derive(Debug, Deserialize, Default)]
struct CreateInvitationRequest {
    ttl_secs: Option<i64>,
}

#[derive(Debug, Serialize)]
struct CreatedInvitation {
    id: Uuid,
    token: String,
    expires_at: chrono::DateTime<Utc>,
}

async fn create_invitation(
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

async fn revoke_invitation(
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
#[derive(Debug, Serialize)]
struct RemuxJobView {
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

async fn list_remux_jobs(
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

#[derive(Debug, Serialize)]
struct WipeRemuxResponse {
    /// Bytes freed by removing the cache file.
    freed_bytes: u64,
}

async fn wipe_remux_job(
    State(state): State<AppState>,
    _admin: AdminUser,
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
    Ok(Json(WipeRemuxResponse {
        freed_bytes: freed,
    }))
}
