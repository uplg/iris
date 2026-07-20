use std::sync::OnceLock;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::post;
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};
use chrono::Duration;
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

/// A refresh token rotated this many seconds ago is still honoured as a
/// straggler (see [`refresh`]): when several clients refresh near-simultaneously
/// the first rotates the token and the rest arrive holding the now-revoked jti.
/// Re-issuing for them — instead of 401-ing — is what stops a multi-tab / retry
/// race from spuriously logging the user out. Bounded so a genuinely stale
/// token can't be replayed long after the fact.
const REFRESH_ROTATION_GRACE_SECS: i64 = 60;

/// Argon2-bearing endpoints: `login`/`register` each run a ~100 ms password
/// hash, so this is the brute-force / CPU-exhaustion surface. Gets the TIGHT
/// rate-limit lane (see `app.rs`).
pub fn strict_router() -> Router<AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
}

/// Cheap, idempotent session endpoints, protected by opaque tokens rather than
/// passwords and hit by every client on a routine cadence (silent re-auth,
/// keep-alive). Gets the GENEROUS rate-limit lane — a 429 on `/refresh` logs
/// the user out, so it must never fire under normal multi-tab / multi-device
/// household load.
pub fn session_router() -> Router<AppState> {
    Router::new()
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

    // Resolve the device tagging to carry forward, tolerating a rotation race.
    // Normal path: the jti is active → rotate it (`mark_rotated`, not `revoke`,
    // so a straggler can still be recognised below). Race path: the jti isn't
    // active but was rotated within the grace window → a near-simultaneous
    // refresh already rotated it, the session is alive, so re-issue instead of
    // logging the user out. An explicitly revoked token (logout / device
    // revoke; `rotated_at` IS NULL) matches neither branch and still 401s.
    //
    // Device tagging is carried across the rotation so a paired TV keeps its
    // `device_kind` (else it drops off the account device list), and devices
    // get the SLIDING full device TTL re-issued on every refresh — a TV in
    // regular use never expires; only one left off longer than the whole window
    // needs re-pairing. Browsers keep `None` (the default, also re-issued).
    let (device_label, device_kind) = if let Some(prev) =
        iris_db::refresh_tokens::get_active_device_info(state.db(), claims.jti).await?
    {
        iris_db::refresh_tokens::mark_rotated(state.db(), claims.jti).await?;
        (prev.device_label, prev.device_kind)
    } else if let Some(rot) = iris_db::refresh_tokens::recently_rotated(
        state.db(),
        claims.jti,
        REFRESH_ROTATION_GRACE_SECS,
    )
    .await?
    {
        // Straggler from a near-simultaneous rotation — the session is alive.
        tracing::debug!(jti = %claims.jti, "refresh straggler within rotation grace; re-issuing");
        (rot.device_label, rot.device_kind)
    } else {
        tracing::warn!(jti = %claims.jti, "refresh rejected: refresh-token row not active (revoked/rotated/expired)");
        return Err(ApiError::Unauthorized);
    };

    let user_id = UserId::from(claims.sub);
    let user = iris_db::users::find_by_id(state.db(), user_id)
        .await?
        .ok_or(ApiError::Unauthorized)?;

    let ttl_override = if device_kind.is_some() {
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
        device_label.as_deref(),
        device_kind.as_deref(),
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
    // The override TTL must reach the JWT encoder itself: `verify_refresh`
    // checks the token's `exp` before any DB lookup, so the JWT, the DB
    // `expires_at` and the cookie Max-Age have to agree on the horizon.
    let (refresh, jti, exp) = state
        .jwt()
        .issue_refresh(user_id, refresh_ttl_override_secs.map(Duration::seconds))
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("issue refresh: {e}")))?;

    iris_db::refresh_tokens::insert_with_device(
        state.db(),
        jti,
        user_id,
        exp,
        device_label,
        device_kind,
    )
    .await?;

    let secure = state.cfg().cookie_secure();
    let access_cookie = build_cookie(
        ACCESS_COOKIE,
        access,
        Duration::seconds(state.cfg().auth.access_ttl_secs),
        "/",
        secure,
    );
    let refresh_cookie = build_cookie(
        REFRESH_COOKIE,
        refresh,
        Duration::seconds(refresh_ttl),
        "/api/auth",
        secure,
    );

    let _ = is_admin; // claim already encoded in the access token
    Ok(jar.clone().add(access_cookie).add(refresh_cookie))
}

fn build_cookie(
    name: &'static str,
    value: String,
    ttl: Duration,
    path: &'static str,
    secure: bool,
) -> Cookie<'static> {
    Cookie::build((name, value))
        .http_only(true)
        .same_site(SameSite::Lax)
        // `Secure` derived from the public URL scheme (config: auth.cookie_secure):
        // on behind the TLS tunnel, off for http://localhost dev so login works.
        .secure(secure)
        .path(path)
        // Max-Age, not an absolute Expires: a client whose clock runs behind
        // the server's would treat a freshly-set Expires cookie as already
        // stale and drop it immediately, logging the user straight back out.
        // Max-Age is a relative duration anchored to the browser's own clock,
        // so it is immune to skew.
        .max_age(time::Duration::seconds(ttl.num_seconds()))
        .build()
}
