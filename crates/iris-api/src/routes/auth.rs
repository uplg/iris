use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::post;
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};
use chrono::Duration;
use iris_auth::hash_invitation_token;
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

impl From<iris_core::user::User> for UserResponse {
    fn from(user: iris_core::user::User) -> Self {
        Self {
            id: user.id.into(),
            email: user.email,
            display_name: user.display_name,
            is_admin: user.is_admin,
        }
    }
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
    crate::passwords::check_policy(&req.password)?;
    let email = normalize_email(&req.email);
    if !email.contains('@') || email.len() < 3 {
        return Err(ApiError::BadRequest("invalid email".into()));
    }

    let hashed_invite = hash_invitation_token(&req.invite_token);
    // Argon2 is slow — do it OUTSIDE the tx so we don't hold a
    // connection for ~100ms.
    let pw_hash = crate::passwords::hash(&req.password).await?;

    // All DB writes in one transaction: previously a race could leave
    // a `users` row created while `invitations::consume` failed (the
    // invite got used between the lookup and the consume), bricking
    // the email forever with no usable account.
    // IMMEDIATE: the transaction reads before it writes, and a deferred one
    // whose snapshot went stale fails its first write with SQLITE_BUSY
    // without waiting (double-clicked register, a concurrent write).
    let mut tx = state.db().begin_with("BEGIN IMMEDIATE").await?;

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
    .await
    .map_err(|e| match e.as_database_error() {
        Some(db) if db.is_unique_violation() => {
            ApiError::Conflict("email already registered".into())
        }
        _ => e.into(),
    })?;

    let consumed =
        iris_db::invitations::consume(&mut *tx, InvitationId::from(invitation.id), user.id).await?;
    if !consumed {
        // Drop without commit → tx rolls back, the `users` insert is undone.
        return Err(ApiError::Conflict("invitation already used".into()));
    }

    tx.commit().await?;

    let jar = issue_session(&state, &jar, user.id, user.is_admin).await?;
    Ok((jar, Json(user.into())))
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
        crate::passwords::verify_nobody(&req.password).await;
        return Err(ApiError::Unauthorized);
    };
    if !crate::passwords::verify(&req.password, &hash).await? {
        return Err(ApiError::Unauthorized);
    }

    let jar = issue_session(&state, &jar, user.id, user.is_admin).await?;
    Ok((jar, Json(user.into())))
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
    state
        .session_cuts()
        .check(state.db(), UserId::from(claims.sub), claims.issued_at_ms())
        .await?;

    // Resolve the device tagging to carry forward, tolerating a rotation race.
    // Normal path: the jti is active → rotate it (`mark_rotated`, not `revoke`,
    // so a straggler can still be recognised below), atomically. Race path: the jti isn't
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
        iris_db::refresh_tokens::mark_rotated(state.db(), claims.jti).await?
    {
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

    Ok((jar, Json(user.into())))
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
    Ok(clear_session(jar))
}

/// The jar with this browser's session cookies removed.
pub(crate) fn clear_session(jar: CookieJar) -> CookieJar {
    jar.remove(Cookie::build(ACCESS_COOKIE).path("/").build())
        .remove(Cookie::build(REFRESH_COOKIE).path("/api/auth").build())
}

pub(crate) async fn issue_session(
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
    issue_device_session(
        state,
        jar,
        user_id,
        is_admin,
        refresh_ttl_override_secs,
        device_label,
        device_kind,
    )
    .await
    .map(|(jar, _)| jar)
}

/// [`issue_session_for_kind`], also answering the new refresh session's id.
pub async fn issue_device_session(
    state: &AppState,
    jar: &CookieJar,
    user_id: UserId,
    is_admin: bool,
    refresh_ttl_override_secs: Option<i64>,
    device_label: Option<&str>,
    device_kind: Option<&str>,
) -> ApiResult<(CookieJar, Uuid)> {
    let access = state
        .jwt()
        .issue_access(user_id, is_admin)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("issue access: {e}")))?;
    let refresh_ttl = refresh_ttl_override_secs.unwrap_or(state.cfg().auth.refresh_ttl_secs);
    // The override TTL must reach the JWT encoder itself: `verify_refresh`
    // checks the token's `exp` before any DB lookup, so the JWT, the DB
    // `expires_at` and the cookie Max-Age have to agree on the horizon.
    let issued = state
        .jwt()
        .issue_refresh(user_id, refresh_ttl_override_secs.map(Duration::seconds))
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("issue refresh: {e}")))?;
    let (refresh, jti) = (issued.token, issued.jti);

    iris_db::refresh_tokens::insert_with_device(
        state.db(),
        jti,
        user_id,
        issued.expires_at,
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

    Ok((jar.clone().add(access_cookie).add(refresh_cookie), jti))
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

#[cfg(test)]
pub(crate) mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use iris_core::ids::UserId;
    use iris_providers::ProviderRegistry;
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use crate::state::AppState;

    pub(crate) struct Reply {
        pub status: StatusCode,
        pub json: Value,
        /// `name=value` of every cookie the response sets.
        pub cookies: Vec<String>,
    }

    impl Reply {
        pub fn cookie(&self, name: &str) -> Option<String> {
            self.cookies
                .iter()
                .find(|c| c.starts_with(&format!("{name}=")))
                .cloned()
        }
    }

    pub(crate) async fn call(
        app: &axum::Router,
        method: &str,
        path: &str,
        bearer: Option<&str>,
        cookie: Option<&str>,
        body: Option<Value>,
    ) -> Reply {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(t) = bearer {
            req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        if let Some(c) = cookie {
            req = req.header(header::COOKIE, c);
        }
        let req = match body {
            Some(b) => req
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(b.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let cookies = res
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .filter_map(|v| v.split(';').next())
            .filter(|kv| !kv.ends_with('='))
            .map(str::to_owned)
            .collect();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        Reply {
            status,
            json: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            cookies,
        }
    }

    pub(crate) const PASSWORD: &str = "correct horse battery";

    /// A router over a fresh DB holding one member who signs in with
    /// [`PASSWORD`].
    pub(crate) async fn app_with_member() -> (AppState, axum::Router, UserId, String) {
        let pool = iris_db::test_support::migrated_pool().await;
        let email = "ana@example.org".to_owned();
        let user = iris_db::users::create(
            &pool,
            iris_db::users::NewUser {
                email: email.clone(),
                password_hash: crate::passwords::hash(PASSWORD).await.unwrap(),
                is_admin: false,
            },
        )
        .await
        .unwrap();
        let state = AppState::for_tests(pool, ProviderRegistry::from_entries(&[]).unwrap()).await;
        let app = crate::app::build_router(state.clone());
        (state, app, user.id, email)
    }

    pub(crate) async fn login(app: &axum::Router, email: &str, password: &str) -> Reply {
        call(
            app,
            "POST",
            "/api/auth/login",
            None,
            None,
            Some(json!({ "email": email, "password": password })),
        )
        .await
    }

    #[tokio::test]
    async fn a_password_change_ends_every_session_the_callers_included() {
        let (state, app, user, email) = app_with_member().await;
        let signed_in = login(&app, &email, PASSWORD).await;
        assert_eq!(signed_in.status, StatusCode::OK);
        let refresh = signed_in.cookie("iris_refresh").unwrap();
        let old = state.jwt().issue_access(user, false).unwrap();
        assert_eq!(
            call(&app, "GET", "/api/me", Some(&old), None, None)
                .await
                .status,
            StatusCode::OK
        );
        let code = iris_db::device_codes::create(
            state.db(),
            "ABCD-2345",
            chrono::Utc::now() + chrono::Duration::minutes(10),
            "android-tv",
        )
        .await
        .unwrap();
        assert!(
            iris_db::device_codes::claim(state.db(), "ABCD-2345", user, None)
                .await
                .unwrap()
        );

        let changed = call(
            &app,
            "POST",
            "/api/me/password",
            Some(&old),
            None,
            Some(json!({ "old_password": PASSWORD, "new_password": "a brand new secret" })),
        )
        .await;
        assert_eq!(changed.status, StatusCode::NO_CONTENT);

        for (method, path, body) in [
            ("GET", "/api/me", None),
            ("POST", "/api/me/passkeys/register/start", Some(json!({}))),
            (
                "POST",
                "/api/me/devices",
                Some(json!({ "code": "WXYZ-2345" })),
            ),
        ] {
            assert_eq!(
                call(&app, method, path, Some(&old), None, body)
                    .await
                    .status,
                StatusCode::UNAUTHORIZED,
                "{method} {path} with a token from before the change"
            );
        }
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/auth/refresh",
                None,
                Some(&refresh),
                None
            )
            .await
            .status,
            StatusCode::UNAUTHORIZED
        );
        let polled = call(
            &app,
            "GET",
            &format!("/api/auth/device/poll/{}", code.device_id),
            None,
            None,
            None,
        )
        .await;
        assert_eq!(polled.json["status"], "expired", "{:?}", polled.json);
        assert!(polled.cookie("iris_refresh").is_none());

        assert_eq!(
            login(&app, &email, PASSWORD).await.status,
            StatusCode::UNAUTHORIZED
        );
        let again = login(&app, &email, "a brand new secret").await;
        assert_eq!(again.status, StatusCode::OK);
        let fresh = state.jwt().issue_access(user, false).unwrap();
        assert_eq!(
            call(&app, "GET", "/api/me", Some(&fresh), None, None)
                .await
                .status,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn a_deleted_account_loses_its_access_token_at_once() {
        let (state, app, user, _) = app_with_member().await;
        let admin = iris_db::test_support::make_user(state.db()).await;
        sqlx::query("UPDATE users SET is_admin = 1 WHERE id = ?1")
            .bind(uuid::Uuid::from(admin))
            .execute(state.db())
            .await
            .unwrap();
        let admin_token = state.jwt().issue_access(admin, true).unwrap();
        let token = state.jwt().issue_access(user, false).unwrap();
        assert_eq!(
            call(&app, "GET", "/api/me", Some(&token), None, None)
                .await
                .status,
            StatusCode::OK
        );
        let deleted = call(
            &app,
            "DELETE",
            &format!("/api/admin/users/{}", uuid::Uuid::from(user)),
            Some(&admin_token),
            None,
            None,
        )
        .await;
        assert_eq!(deleted.status, StatusCode::NO_CONTENT);
        assert_eq!(
            call(&app, "GET", "/api/me", Some(&token), None, None)
                .await
                .status,
            StatusCode::UNAUTHORIZED
        );
    }
}
