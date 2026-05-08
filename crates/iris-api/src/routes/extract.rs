//! Auth extractors: pull the access token from `Authorization: Bearer …`
//! or the `iris_access` cookie, verify, and inject the user.

use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;
use axum_extra::extract::CookieJar;
use iris_auth::jwt::AccessClaims;
use iris_core::ids::UserId;
use uuid::Uuid;

use crate::error::ApiError;
use crate::state::AppState;

pub const ACCESS_COOKIE: &str = "iris_access";
pub const REFRESH_COOKIE: &str = "iris_refresh";

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: UserId,
    pub is_admin: bool,
    pub claims: AccessClaims,
}

impl<S> FromRequestParts<S> for AuthUser
where
    AppState: axum::extract::FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app: AppState = AppState::from_ref(state);

        let header_token = parts
            .headers
            .get(http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer ").map(str::to_owned));

        let token = if let Some(t) = header_token {
            t
        } else {
            let jar = CookieJar::from_headers(&parts.headers);
            jar.get(ACCESS_COOKIE)
                .map(|c| c.value().to_owned())
                .ok_or(ApiError::Unauthorized)?
        };

        let claims = app.jwt().verify_access(&token).map_err(|e| {
            tracing::debug!(error = %e, "access token verify failed");
            ApiError::Unauthorized
        })?;

        Ok(Self {
            id: UserId::from(Uuid::from(claims.sub)),
            is_admin: claims.admin,
            claims,
        })
    }
}

#[derive(Debug, Clone)]
pub struct AdminUser(pub AuthUser);

impl<S> FromRequestParts<S> for AdminUser
where
    AppState: axum::extract::FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let user = AuthUser::from_request_parts(parts, state).await?;
        if !user.is_admin {
            return Err(ApiError::Forbidden);
        }
        Ok(Self(user))
    }
}
