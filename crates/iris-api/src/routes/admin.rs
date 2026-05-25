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
        .route("/tmdb/diagnose/{infohash}", get(diagnose_tmdb))
        .route("/active-sessions", get(active_sessions))
        .route("/watch-history", get(watch_history))
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
#[derive(Debug, Serialize)]
struct ActiveSessionView {
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
    kind: Option<String>,
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

async fn active_sessions(
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
            kind: card.as_ref().and_then(|c| c.kind.clone()),
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
#[derive(Debug, Serialize)]
struct WatchHistoryView {
    user_id: Uuid,
    display_name: String,
    infohash: String,
    file_idx: i64,
    torrent_name: String,
    /// On-disk path of the watched file (episode within a season pack).
    file_path: Option<String>,
    tmdb_id: Option<i64>,
    tmdb_verified: bool,
    kind: Option<String>,
    position_seconds: f64,
    duration_seconds: Option<f64>,
    completed: bool,
    last_watched_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct WatchHistoryQuery {
    limit: Option<i64>,
}

async fn watch_history(
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
                kind: r.kind,
                position_seconds: r.position_seconds,
                duration_seconds: r.duration_seconds,
                completed: r.completed,
                last_watched_at: r.last_watched_at,
            })
            .collect(),
    ))
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
#[derive(Debug, Serialize)]
struct TmdbDiagnose {
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

#[derive(Debug, Serialize)]
struct TmdbDiagnoseParsed {
    title: String,
    year: Option<u16>,
    season: Option<u32>,
    episode: Option<u32>,
    is_tv: bool,
}

#[derive(Debug, Serialize)]
struct TmdbDiagnoseSuggestion {
    kind: String,
    tmdb_id: u64,
    title: String,
    year: Option<u32>,
    poster_path: Option<String>,
}

async fn diagnose_tmdb(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(infohash): Path<String>,
) -> ApiResult<Json<TmdbDiagnose>> {
    let infohash = infohash.to_ascii_lowercase();
    let row = iris_db::torrents::find_by_infohash(state.db(), &infohash)
        .await?
        .ok_or(ApiError::NotFound)?;

    let collection_tmdb_id = match row.collection_id {
        Some(cid) => iris_db::collections::get(state.db(), cid).await?.and_then(|c| c.tmdb_id),
        None => None,
    };

    let parsed = iris_media::filename::parse(&row.name);
    let cleaned = parsed
        .as_ref()
        .map(|p| iris_media::filename::series_key(&p.title))
        .unwrap_or_default();

    let mut suggestions: Vec<TmdbDiagnoseSuggestion> = Vec::new();
    let mut picked: Option<TmdbDiagnoseSuggestion> = None;

    if let (Some(tmdb), Some(p)) = (state.tmdb(), parsed.as_ref()) {
        if cleaned.len() >= 2 {
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
            if let Some(r) =
                crate::tmdb_resolve::resolve_cleaned(state.db(), tmdb, &cleaned, kind_hint, p.year.map(u32::from)).await
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
