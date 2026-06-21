//! Device pairing endpoints.
//!
//! Flow:
//! - The headless client (Android TV) hits `POST /auth/device/code` and gets
//!   a short `code` it shows on screen plus an opaque `device_id` it polls
//!   on.
//! - The user opens the web UI, signs in normally, and `POST`s the code on
//!   `/me/devices/link` together with a friendly label.
//! - The TV's poll on `GET /auth/device/poll/:device_id` flips from
//!   `pending` to `linked` and receives access + refresh cookies just like
//!   `/auth/login`.

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum_extra::extract::CookieJar;
use chrono::{Duration, Utc};
use iris_core::ids::UserId;
use rand::seq::IteratorRandom;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::routes::auth::issue_session_for_kind;
use crate::routes::extract::AuthUser;
use crate::state::AppState;

pub fn auth_router() -> Router<AppState> {
    Router::new()
        .route("/code", axum::routing::post(create_code))
        .route("/poll/{device_id}", axum::routing::get(poll))
}

pub fn me_router() -> Router<AppState> {
    Router::new()
        .route("/", axum::routing::get(list).post(link))
        .route("/{jti}", axum::routing::delete(revoke))
}

const DEVICE_CODE_TTL_SECS: i64 = 600; // 10 minutes

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateCodeRequest {
    #[serde(default = "default_kind")]
    pub kind: String,
}

fn default_kind() -> String {
    "unknown".into()
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreateCodeResponse {
    pub code: String,
    pub device_id: Uuid,
    pub verification_url: String,
    pub expires_in: i64,
}

#[utoipa::path(
    post,
    path = "/api/auth/device/code",
    request_body = CreateCodeRequest,
    responses((status = 200, description = "Pairing code + opaque device id to poll", body = CreateCodeResponse)),
    tag = "devices",
)]
pub(crate) async fn create_code(
    State(state): State<AppState>,
    Json(req): Json<CreateCodeRequest>,
) -> ApiResult<Json<CreateCodeResponse>> {
    // Best-effort cleanup of stale codes — no big deal if it errors.
    let _ = iris_db::device_codes::cleanup_expired(state.db()).await;

    let code = generate_code();
    let expires_at = Utc::now() + Duration::seconds(DEVICE_CODE_TTL_SECS);
    let row = iris_db::device_codes::create(state.db(), &code, expires_at, &req.kind).await?;

    let public_url = state.cfg().server.public_url.trim_end_matches('/');
    let verification_url = format!("{public_url}/account?pair={}", row.code);
    Ok(Json(CreateCodeResponse {
        code: row.code,
        device_id: row.device_id,
        verification_url,
        expires_in: DEVICE_CODE_TTL_SECS,
    }))
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PollResponse {
    Pending,
    Expired,
    Linked { user: PolledUser },
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PolledUser {
    pub id: Uuid,
    pub email: String,
    pub is_admin: bool,
}

/// Poll a device-pairing code's status. `pending` until the user links it
/// from the web UI, then `linked` with the user payload (and the session
/// cookies are set), or `expired`.
#[utoipa::path(
    get,
    path = "/api/auth/device/poll/{device_id}",
    params(
        ("device_id" = Uuid, Path, description = "Opaque id returned by POST /auth/device/code"),
    ),
    responses(
        (status = 200, description = "Current pairing status", body = PollResponse),
        (status = 404, description = "Unknown device id"),
    ),
    tag = "devices",
)]
pub(crate) async fn poll(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(device_id): Path<Uuid>,
) -> ApiResult<(CookieJar, Json<PollResponse>)> {
    let row = iris_db::device_codes::find_by_device_id(state.db(), device_id)
        .await?
        .ok_or(ApiError::NotFound)?;

    // Expiry bounds session issuance even after the code is CLAIMED. A claimed
    // code re-issues a session on every poll, so without checking expiry here a
    // client that never stops polling (an old/misbehaving APK) would mint an
    // unbounded number of refresh-token rows. The 10-min code TTL caps that;
    // past it the device must re-pair.
    if row.expires_at < Utc::now() {
        return Ok((jar, Json(PollResponse::Expired)));
    }

    let Some(user_id) = row.claimed_by else {
        return Ok((jar, Json(PollResponse::Pending)));
    };

    let user = iris_db::users::find_by_id(state.db(), UserId::from(user_id))
        .await?
        .ok_or(ApiError::NotFound)?;

    // Hand the device back a real session via the same cookie path the web
    // login uses, with a longer refresh TTL and labelled with the device
    // kind so the user can revoke it in their account UI.
    let jar = issue_session_for_kind(
        &state,
        &jar,
        user.id,
        user.is_admin,
        Some(state.cfg().auth.device_refresh_ttl_secs),
        row.label.as_deref(),
        Some(&row.kind),
    )
    .await?;

    Ok((
        jar,
        Json(PollResponse::Linked {
            user: PolledUser {
                id: user.id.into(),
                email: user.email,
                is_admin: user.is_admin,
            },
        }),
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LinkRequest {
    pub code: String,
    pub label: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/me/devices",
    request_body = LinkRequest,
    responses(
        (status = 204, description = "Code linked to the caller's account"),
        (status = 400, description = "Invalid or expired code"),
        (status = 409, description = "Code already claimed"),
    ),
    tag = "devices",
)]
pub(crate) async fn link(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<LinkRequest>,
) -> ApiResult<axum::http::StatusCode> {
    let code = req.code.trim().to_ascii_uppercase();
    let active = iris_db::device_codes::find_active_by_code(state.db(), &code)
        .await?
        .ok_or_else(|| ApiError::BadRequest("invalid or expired code".into()))?;
    let _ = active;
    let label = req
        .label
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let claimed = iris_db::device_codes::claim(state.db(), &code, user.id, label).await?;
    if !claimed {
        return Err(ApiError::Conflict("code already claimed or expired".into()));
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeviceView {
    pub jti: Uuid,
    pub label: Option<String>,
    pub kind: Option<String>,
    pub issued_at: chrono::DateTime<Utc>,
    pub expires_at: chrono::DateTime<Utc>,
}

#[utoipa::path(
    get,
    path = "/api/me/devices",
    operation_id = "list_devices",
    responses((status = 200, description = "The caller's linked devices", body = [DeviceView])),
    tag = "devices",
)]
pub(crate) async fn list(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<DeviceView>>> {
    let rows = iris_db::refresh_tokens::list_devices_for_user(state.db(), user.id).await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| DeviceView {
                jti: r.jti,
                label: r.device_label,
                kind: r.device_kind,
                issued_at: r.issued_at,
                expires_at: r.expires_at,
            })
            .collect(),
    ))
}

#[utoipa::path(
    delete,
    path = "/api/me/devices/{jti}",
    params(("jti" = Uuid, Path, description = "Refresh-token id of the device to revoke")),
    responses(
        (status = 204, description = "Device revoked"),
        (status = 404, description = "No such device for this user"),
    ),
    tag = "devices",
)]
pub(crate) async fn revoke(
    State(state): State<AppState>,
    user: AuthUser,
    Path(jti): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    let ok = iris_db::refresh_tokens::revoke_for_user(state.db(), user.id, jti).await?;
    if ok {
        Ok(axum::http::StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

/// 8-character user-facing pairing code, alphabet trimmed of confusable
/// glyphs (no 0/O, no 1/I/L).
fn generate_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let mut rng = rand::rng();
    let take = |rng: &mut _, n: usize| {
        (0..n)
            .map(|_| char::from(*ALPHABET.iter().choose(rng).unwrap()))
            .collect::<String>()
    };
    let head = take(&mut rng, 4);
    let tail = take(&mut rng, 4);
    format!("{head}-{tail}")
}
