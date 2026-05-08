use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::post;
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};
use chrono::{Duration, Utc};
use iris_auth::{hash_password, hash_invitation_token, verify_password};
use iris_core::ids::{InvitationId, UserId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::{ACCESS_COOKIE, REFRESH_COOKIE};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/refresh", post(refresh))
        .route("/logout", post(logout))
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub invite_token: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub email: String,
    pub is_admin: bool,
}

async fn register(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<RegisterRequest>,
) -> ApiResult<(CookieJar, Json<UserResponse>)> {
    if req.password.len() < 8 {
        return Err(ApiError::BadRequest("password too short (min 8)".into()));
    }
    if !req.email.contains('@') {
        return Err(ApiError::BadRequest("invalid email".into()));
    }

    let hashed_invite = hash_invitation_token(&req.invite_token);
    let invitation = iris_db::invitations::find_active_by_hash(state.db(), &hashed_invite)
        .await?
        .ok_or_else(|| ApiError::BadRequest("invalid or expired invitation".into()))?;

    if iris_db::users::find_by_email(state.db(), &req.email)
        .await?
        .is_some()
    {
        return Err(ApiError::Conflict("email already registered".into()));
    }

    let pw_hash = hash_password(&req.password)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("hash: {e}")))?;
    let user = iris_db::users::create(
        state.db(),
        iris_db::users::NewUser {
            email: req.email.clone(),
            password_hash: pw_hash,
            is_admin: false,
        },
    )
    .await?;

    let consumed = iris_db::invitations::consume(
        state.db(),
        InvitationId::from(invitation.id),
        user.id,
    )
    .await?;
    if !consumed {
        return Err(ApiError::Conflict("invitation already used".into()));
    }

    let jar = issue_session(&state, &jar, user.id, user.is_admin).await?;
    Ok((
        jar,
        Json(UserResponse {
            id: user.id.into(),
            email: user.email,
            is_admin: user.is_admin,
        }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<LoginRequest>,
) -> ApiResult<(CookieJar, Json<UserResponse>)> {
    let (user, hash) = iris_db::users::find_by_email(state.db(), &req.email)
        .await?
        .ok_or(ApiError::Unauthorized)?;

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
            is_admin: user.is_admin,
        }),
    ))
}

async fn refresh(
    State(state): State<AppState>,
    jar: CookieJar,
) -> ApiResult<(CookieJar, Json<UserResponse>)> {
    let token = jar
        .get(REFRESH_COOKIE)
        .map(|c| c.value().to_owned())
        .ok_or(ApiError::Unauthorized)?;

    let claims = state
        .jwt()
        .verify_refresh(&token)
        .map_err(|_| ApiError::Unauthorized)?;

    if !iris_db::refresh_tokens::is_active(state.db(), claims.jti).await? {
        return Err(ApiError::Unauthorized);
    }

    let user_id = UserId::from(Uuid::from(claims.sub));
    let user = iris_db::users::find_by_id(state.db(), user_id)
        .await?
        .ok_or(ApiError::Unauthorized)?;

    iris_db::refresh_tokens::revoke(state.db(), claims.jti).await?;
    let jar = issue_session(&state, &jar, user.id, user.is_admin).await?;

    Ok((
        jar,
        Json(UserResponse {
            id: user.id.into(),
            email: user.email,
            is_admin: user.is_admin,
        }),
    ))
}

async fn logout(
    State(state): State<AppState>,
    jar: CookieJar,
) -> ApiResult<CookieJar> {
    if let Some(token) = jar.get(REFRESH_COOKIE).map(|c| c.value().to_owned()) {
        if let Ok(claims) = state.jwt().verify_refresh(&token) {
            iris_db::refresh_tokens::revoke(state.db(), claims.jti).await?;
        }
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
    let access = state
        .jwt()
        .issue_access(user_id, is_admin)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("issue access: {e}")))?;
    let (refresh, jti, exp) = state
        .jwt()
        .issue_refresh(user_id)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("issue refresh: {e}")))?;

    iris_db::refresh_tokens::insert(state.db(), jti, user_id, exp).await?;

    let access_cookie = build_cookie(
        ACCESS_COOKIE,
        access,
        Duration::seconds(state.cfg().auth.access_ttl_secs),
        "/",
    );
    let refresh_cookie = build_cookie(
        REFRESH_COOKIE,
        refresh,
        Duration::seconds(state.cfg().auth.refresh_ttl_secs),
        "/api/auth",
    );

    Ok(jar.clone().add(access_cookie).add(refresh_cookie))
}

fn build_cookie(name: &'static str, value: String, ttl: Duration, path: &'static str) -> Cookie<'static> {
    let expires = Utc::now() + ttl;
    Cookie::build((name, value))
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(false) // flipped to true behind TLS reverse proxy via config later
        .path(path)
        .expires(time::OffsetDateTime::from_unix_timestamp(expires.timestamp()).unwrap())
        .build()
}
