use std::sync::OnceLock;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::post;
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};
use chrono::{Duration, Utc};
use iris_auth::{hash_invitation_token, hash_password, verify_password};
use iris_core::ids::{InvitationId, UserId};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::{ACCESS_COOKIE, REFRESH_COOKIE};
use crate::state::AppState;

/// Canonical form for storage + lookup: trim + lowercase. Prevents
/// `Alice@Example.com` and `alice@example.com` from registering as
/// two distinct accounts on `SQLite`'s case-sensitive `TEXT UNIQUE`.
fn normalize_email(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

/// Lazily-built Argon2 hash of a never-matched password. Used by
/// [`login`] on the "unknown email" branch so the response takes the
/// same wall-clock time as a real verify — without this, an attacker
/// can enumerate valid emails by measuring how quickly we 401.
fn dummy_password_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| {
        hash_password("never-matches-by-design")
            .expect("argon2 hash of a constant input is infallible")
    })
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/refresh", post(refresh))
        .route("/logout", post(logout))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RegisterRequest {
    pub invite_token: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UserResponse {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub is_admin: bool,
}

#[utoipa::path(
    post,
    path = "/api/auth/register",
    request_body = RegisterRequest,
    responses(
        (status = 200, description = "Account created; session cookies set", body = UserResponse),
        (status = 400, description = "Weak password / invalid email"),
        (status = 409, description = "Email already registered or invitation already used"),
    ),
    tag = "auth",
)]
pub(crate) async fn register(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<RegisterRequest>,
) -> ApiResult<(CookieJar, Json<UserResponse>)> {
    if req.password.len() < 8 {
        return Err(ApiError::BadRequest("password too short (min 8)".into()));
    }
    let email = normalize_email(&req.email);
    if !email.contains('@') || email.len() < 3 {
        return Err(ApiError::BadRequest("invalid email".into()));
    }

    let hashed_invite = hash_invitation_token(&req.invite_token);
    // Argon2 is slow — do it OUTSIDE the tx so we don't hold a
    // connection for ~100ms.
    let pw_hash = hash_password(&req.password)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("hash: {e}")))?;

    // All DB writes in one transaction: previously a race could leave
    // a `users` row created while `invitations::consume` failed (the
    // invite got used between the lookup and the consume), bricking
    // the email forever with no usable account.
    let mut tx = state.db().begin().await?;

    let invitation = iris_db::invitations::find_active_by_hash(&mut *tx, &hashed_invite)
        .await?
        .ok_or_else(|| ApiError::BadRequest("invalid or expired invitation".into()))?;

    if iris_db::users::find_by_email(&mut *tx, &email)
        .await?
        .is_some()
    {
        return Err(ApiError::Conflict("email already registered".into()));
    }

    let user = iris_db::users::create(
        &mut *tx,
        iris_db::users::NewUser {
            email: email.clone(),
            password_hash: pw_hash,
            is_admin: false,
        },
    )
    .await?;

    let consumed =
        iris_db::invitations::consume(&mut *tx, InvitationId::from(invitation.id), user.id).await?;
    if !consumed {
        // Drop without commit → tx rolls back, the `users` insert is undone.
        return Err(ApiError::Conflict("invitation already used".into()));
    }

    tx.commit().await?;

    let jar = issue_session(&state, &jar, user.id, user.is_admin).await?;
    Ok((
        jar,
        Json(UserResponse {
            id: user.id.into(),
            email: user.email,
            display_name: user.display_name,
            is_admin: user.is_admin,
        }),
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[utoipa::path(
    post,
    path = "/api/auth/login",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Signed in; session cookies set", body = UserResponse),
        (status = 401, description = "Bad credentials"),
    ),
    tag = "auth",
)]
pub(crate) async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<LoginRequest>,
) -> ApiResult<(CookieJar, Json<UserResponse>)> {
    let email = normalize_email(&req.email);
    let found = iris_db::users::find_by_email(state.db(), &email).await?;

    // Always run the Argon2 verify, even on unknown emails: a real
    // miss took ~0ms (DB short-circuit) while a wrong password took
    // ~100ms, so an attacker could enumerate valid accounts by
    // measuring response time alone. Verifying against a constant
    // dummy hash levels the wall-clock cost in both branches.
    let Some((user, hash)) = found else {
        let _ = verify_password(&req.password, dummy_password_hash());
        return Err(ApiError::Unauthorized);
    };
    let ok = verify_password(&req.password, &hash)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("verify: {e}")))?;
    if !ok {
        return Err(ApiError::Unauthorized);
    }

    let jar = issue_session(&state, &jar, user.id, user.is_admin).await?;
    Ok((
        jar,
        Json(UserResponse {
            id: user.id.into(),
            email: user.email,
            display_name: user.display_name,
            is_admin: user.is_admin,
        }),
    ))
}

#[utoipa::path(
    post,
    path = "/api/auth/refresh",
    responses(
        (status = 200, description = "Session rotated; new cookies set", body = UserResponse),
        (status = 401, description = "Missing / expired / revoked refresh token"),
    ),
    tag = "auth",
)]
pub(crate) async fn refresh(
    State(state): State<AppState>,
    jar: CookieJar,
) -> ApiResult<(CookieJar, Json<UserResponse>)> {
    let token = jar
        .get(REFRESH_COOKIE)
        .map(|c| c.value().to_owned())
        .ok_or(ApiError::Unauthorized)?;

    let claims = state.jwt().verify_refresh(&token).map_err(|e| {
        // Distinguishes a JWT-level failure (expired token / bad signature /
        // rotated server secret) from a DB-level revocation below. Without
        // this, an early "401 that never reconnects" on a paired TV is
        // undiagnosable — we can't tell whether the token aged out, was
        // rotated away, or the secret changed under it.
        tracing::warn!(error = %e, "refresh rejected: token verify failed");
        ApiError::Unauthorized
    })?;

    let Some(prev) =
        iris_db::refresh_tokens::get_active_device_info(state.db(), claims.jti).await?
    else {
        // Token is well-formed but not active in the DB: already rotated
        // (its jti was revoked by a prior refresh), explicitly revoked, or
        // expired server-side. This is the branch a rotation race / double
        // refresh would hit.
        tracing::warn!(jti = %claims.jti, "refresh rejected: refresh-token row not active (revoked/rotated/expired)");
        return Err(ApiError::Unauthorized);
    };

    let user_id = UserId::from(claims.sub);
    let user = iris_db::users::find_by_id(state.db(), user_id)
        .await?
        .ok_or(ApiError::Unauthorized)?;

    iris_db::refresh_tokens::revoke(state.db(), claims.jti).await?;
    // Carry the device tagging forward across the rotation. Without this,
    // a paired TV's first refresh strips `device_kind` from the row and
    // it disappears from the account-page device list.
    //
    // SLIDING window for devices: re-issue the FULL device TTL on every
    // refresh rather than carrying the remaining lifetime. The old code
    // passed `remaining_ttl`, which pinned the expiry to the original pairing
    // — so a TV in daily use still hard-expired at pairing+90d and dumped the
    // user onto a dead "401 / Retry" screen. A fresh full window means a TV in
    // regular use never expires; only one left off for longer than the entire
    // window needs re-pairing. Browsers keep `None` (the configured default,
    // which is already re-issued fresh each refresh → also sliding).
    let ttl_override = if prev.device_kind.is_some() {
        Some(state.cfg().auth.device_refresh_ttl_secs)
    } else {
        None
    };
    let jar = issue_session_for_kind(
        &state,
        &jar,
        user.id,
        user.is_admin,
        ttl_override,
        prev.device_label.as_deref(),
        prev.device_kind.as_deref(),
    )
    .await?;

    Ok((
        jar,
        Json(UserResponse {
            id: user.id.into(),
            email: user.email,
            display_name: user.display_name,
            is_admin: user.is_admin,
        }),
    ))
}

#[utoipa::path(
    post,
    path = "/api/auth/logout",
    responses((status = 200, description = "Session revoked; cookies cleared")),
    tag = "auth",
)]
pub(crate) async fn logout(State(state): State<AppState>, jar: CookieJar) -> ApiResult<CookieJar> {
    if let Some(token) = jar.get(REFRESH_COOKIE).map(|c| c.value().to_owned())
        && let Ok(claims) = state.jwt().verify_refresh(&token)
    {
        iris_db::refresh_tokens::revoke(state.db(), claims.jti).await?;
    }
    let jar = jar
        .remove(Cookie::build(ACCESS_COOKIE).path("/").build())
        .remove(Cookie::build(REFRESH_COOKIE).path("/api/auth").build());
    Ok(jar)
}

async fn issue_session(
    state: &AppState,
    jar: &CookieJar,
    user_id: UserId,
    is_admin: bool,
) -> ApiResult<CookieJar> {
    issue_session_for_kind(state, jar, user_id, is_admin, None, None, None).await
}

/// Variant of [`issue_session`] for device-paired sessions: longer refresh
/// TTL, and the refresh-token row is tagged with `device_label` + `device_kind`
/// so we can list/revoke devices in the account UI.
pub async fn issue_session_for_kind(
    state: &AppState,
    jar: &CookieJar,
    user_id: UserId,
    is_admin: bool,
    refresh_ttl_override_secs: Option<i64>,
    device_label: Option<&str>,
    device_kind: Option<&str>,
) -> ApiResult<CookieJar> {
    let access = state
        .jwt()
        .issue_access(user_id, is_admin)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("issue access: {e}")))?;
    let refresh_ttl = refresh_ttl_override_secs.unwrap_or(state.cfg().auth.refresh_ttl_secs);
    let (refresh, jti, exp) = state
        .jwt()
        .issue_refresh(user_id)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("issue refresh: {e}")))?;
    // exp from issue_refresh uses the configured refresh TTL — recompute when
    // the caller wanted a longer one (devices). Token contents already
    // include exp, so we pass override_exp via the DB row only; the JWT
    // encoder used the default. For our purposes that's acceptable: we use
    // DB-side `expires_at` to invalidate, and we store the device-scoped
    // longer expiration there.
    let exp = if let Some(secs) = refresh_ttl_override_secs {
        chrono::Utc::now() + Duration::seconds(secs)
    } else {
        exp
    };

    iris_db::refresh_tokens::insert_with_device(
        state.db(),
        jti,
        user_id,
        exp,
        device_label,
        device_kind,
    )
    .await?;

    let access_cookie = build_cookie(
        ACCESS_COOKIE,
        access,
        Duration::seconds(state.cfg().auth.access_ttl_secs),
        "/",
    );
    let refresh_cookie = build_cookie(
        REFRESH_COOKIE,
        refresh,
        Duration::seconds(refresh_ttl),
        "/api/auth",
    );

    let _ = is_admin; // claim already encoded in the access token
    Ok(jar.clone().add(access_cookie).add(refresh_cookie))
}

fn build_cookie(
    name: &'static str,
    value: String,
    ttl: Duration,
    path: &'static str,
) -> Cookie<'static> {
    let expires = Utc::now() + ttl;
    Cookie::build((name, value))
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(false) // flipped to true behind TLS reverse proxy via config later
        .path(path)
        .expires(time::OffsetDateTime::from_unix_timestamp(expires.timestamp()).unwrap())
        .build()
}
