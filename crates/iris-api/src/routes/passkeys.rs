//! Passkey routes: usernameless sign-in under `/api/auth/passkey`, and the
//! signed-in user's own passkeys under `/api/me/passkeys`. The ceremonies
//! live in [`crate::passkeys`]; options and credentials travel as the
//! `WebAuthn` JSON forms (`parseCreationOptionsFromJSON` / `toJSON()`).

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::routing::{get, patch, post};
use axum_extra::extract::CookieJar;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use webauthn_rs::prelude::{
    CreationChallengeResponse, PublicKeyCredential, RegisterPublicKeyCredential,
    RequestChallengeResponse,
};

use crate::error::{ApiError, ApiResult};
use crate::rate_limit::ClientIp;
use crate::routes::auth::UserResponse;
use crate::routes::extract::AuthUser;
use crate::state::AppState;

/// Sign-in ceremonies: mounted with the password login, on its strict lane.
pub fn auth_router() -> Router<AppState> {
    Router::new()
        .route("/passkey/login/start", post(login_start))
        .route("/passkey/login/finish", post(login_finish))
}

pub fn me_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/register/start", post(register_start))
        .route("/register/finish", post(register_finish))
        .route("/{id}", patch(rename).delete(remove))
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct LoginStartRequest {
    /// Autofill (`mediation: "conditional"`), or a modal request from a
    /// button.
    #[serde(default)]
    conditional: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct LoginOptions {
    /// Send back with the answer.
    ceremony: String,
    /// For `navigator.credentials.get()`: `publicKey` through
    /// `PublicKeyCredential.parseRequestOptionsFromJSON`, and `mediation`
    /// when conditional.
    #[schema(value_type = Object)]
    options: RequestChallengeResponse,
}

#[utoipa::path(
    post,
    path = "/api/auth/passkey/login/start",
    request_body = LoginStartRequest,
    responses(
        (status = 200, body = LoginOptions),
        (status = 404, description = "Passkeys aren't available on this server"),
    ),
    tag = "passkeys",
)]
pub(crate) async fn login_start(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Json(req): Json<LoginStartRequest>,
) -> ApiResult<Json<LoginOptions>> {
    let (ceremony, options) = state.passkeys()?.login_options(ip, req.conditional)?;
    Ok(Json(LoginOptions { ceremony, options }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct LoginFinishRequest {
    ceremony: String,
    /// `PublicKeyCredential.toJSON()` of the assertion.
    #[schema(value_type = Object)]
    credential: PublicKeyCredential,
}

#[utoipa::path(
    post,
    path = "/api/auth/passkey/login/finish",
    request_body = LoginFinishRequest,
    responses(
        (status = 200, description = "Signed in; session cookies set", body = UserResponse),
        (status = 400, description = "The ceremony expired"),
        (status = 401, description = "Passkey unknown or refused"),
    ),
    tag = "passkeys",
)]
pub(crate) async fn login_finish(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<LoginFinishRequest>,
) -> ApiResult<(CookieJar, Json<UserResponse>)> {
    let user_id = state
        .passkeys()?
        .finish_login(state.db(), &req.ceremony, &req.credential)
        .await?;
    let user = iris_db::users::find_by_id(state.db(), user_id)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    let jar = crate::routes::auth::issue_session(&state, &jar, user.id, user.is_admin).await?;
    Ok((jar, Json(user.into())))
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct PasskeyView {
    id: Uuid,
    name: String,
    /// Synced by the provider (iCloud Keychain, Google Password Manager…)
    /// rather than bound to one device.
    backed_up: bool,
    created_at: DateTime<Utc>,
    last_used_at: Option<DateTime<Utc>>,
}

impl From<iris_db::passkeys::PasskeyRow> for PasskeyView {
    fn from(row: iris_db::passkeys::PasskeyRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            backed_up: row.backed_up,
            created_at: row.created_at,
            last_used_at: row.last_used_at,
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/me/passkeys",
    responses((status = 200, body = [PasskeyView])),
    tag = "passkeys",
)]
pub(crate) async fn list(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<PasskeyView>>> {
    let rows = iris_db::passkeys::list_for_user(state.db(), user.id).await?;
    Ok(Json(rows.into_iter().map(PasskeyView::from).collect()))
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct RegisterOptions {
    ceremony: String,
    /// For `navigator.credentials.create()`: `publicKey` through
    /// `PublicKeyCredential.parseCreationOptionsFromJSON`.
    #[schema(value_type = Object)]
    options: CreationChallengeResponse,
}

#[utoipa::path(
    post,
    path = "/api/me/passkeys/register/start",
    responses(
        (status = 200, body = RegisterOptions),
        (status = 404, description = "Passkeys aren't available on this server"),
    ),
    tag = "passkeys",
)]
pub(crate) async fn register_start(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    user: AuthUser,
) -> ApiResult<Json<RegisterOptions>> {
    let me = iris_db::users::find_by_id(state.db(), user.id)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    let (ceremony, options) = state
        .passkeys()?
        .registration_options(state.db(), ip, &me)
        .await?;
    Ok(Json(RegisterOptions { ceremony, options }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct RegisterFinishRequest {
    ceremony: String,
    /// `PublicKeyCredential.toJSON()` of the new credential.
    #[schema(value_type = Object)]
    credential: RegisterPublicKeyCredential,
    /// What the user calls it; named after the device when absent.
    name: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/me/passkeys/register/finish",
    request_body = RegisterFinishRequest,
    responses(
        (status = 200, body = PasskeyView),
        (status = 400, description = "The ceremony expired"),
        (status = 401, description = "The authenticator's answer was refused"),
    ),
    tag = "passkeys",
)]
pub(crate) async fn register_finish(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Json(req): Json<RegisterFinishRequest>,
) -> ApiResult<Json<PasskeyView>> {
    let name = match req.name.as_deref().map(clean_name).transpose()? {
        Some(n) if !n.is_empty() => n.to_owned(),
        _ => crate::passkeys::device_label(
            headers
                .get(header::USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .unwrap_or(""),
        )
        .to_owned(),
    };
    let row = state
        .passkeys()?
        .finish_registration(state.db(), user.id, &req.ceremony, &req.credential, &name)
        .await?;
    Ok(Json(row.into()))
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct RenamePasskeyRequest {
    name: String,
}

#[utoipa::path(
    patch,
    path = "/api/me/passkeys/{id}",
    params(("id" = Uuid, Path)),
    request_body = RenamePasskeyRequest,
    responses((status = 204), (status = 404, description = "Not one of the caller's passkeys")),
    tag = "passkeys",
)]
pub(crate) async fn rename(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<RenamePasskeyRequest>,
) -> ApiResult<StatusCode> {
    let name = clean_name(&req.name)?;
    if name.is_empty() {
        return Err(ApiError::BadRequest("passkey name cannot be empty".into()));
    }
    if !iris_db::passkeys::rename(state.db(), user.id, id, name).await? {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete,
    path = "/api/me/passkeys/{id}",
    params(("id" = Uuid, Path)),
    responses((status = 204), (status = 404, description = "Not one of the caller's passkeys")),
    tag = "passkeys",
)]
pub(crate) async fn remove(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    if !iris_db::passkeys::delete(state.db(), user.id, id).await? {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// A passkey name as stored: trimmed, at most 64 bytes. Empty is allowed on
/// registration (the device names it).
fn clean_name(raw: &str) -> ApiResult<&str> {
    let trimmed = raw.trim();
    if trimmed.len() > 64 {
        return Err(ApiError::BadRequest(
            "passkey name too long (max 64)".into(),
        ));
    }
    Ok(trimmed)
}
