//! Auth extractors: pull the access token from `Authorization: Bearer …`
//! or the `iris_access` cookie, verify, and inject the user.

use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;
use axum_extra::extract::CookieJar;
use iris_auth::jwt::AccessClaims;
use iris_core::ids::UserId;

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

    fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> {
        // Nothing here awaits — the token is in the headers and verification
        // is pure CPU — so hand axum a ready future instead of an async block
        // that would poll once for nothing.
        std::future::ready(Self::from_parts(parts, &AppState::from_ref(state)))
    }
}

impl AuthUser {
    fn from_parts(parts: &Parts, app: &AppState) -> Result<Self, ApiError> {
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
            id: UserId::from(claims.sub),
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

/// axum's `Path`, answering a malformed segment (a bad infohash, uuid or
/// index) with the API's JSON `bad_request` envelope instead of axum's plain
/// text, so clients show the message like any other error.
#[derive(Debug, Clone, Copy)]
pub struct Path<T>(pub T);

impl<S, T> FromRequestParts<S> for Path<T>
where
    T: serde::de::DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match axum::extract::Path::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Path(value)) => Ok(Self(value)),
            Err(e) if e.status().is_client_error() => Err(ApiError::BadRequest(e.body_text())),
            Err(e) => Err(ApiError::Internal(anyhow::anyhow!(e.body_text()))),
        }
    }
}

/// An `{infohash}` path parameter: a v1 `BitTorrent` infohash (40 hex
/// characters), lowercased — the form librqbit stores and every table keys
/// on. Anything else fails extraction with a 400 before a handler reaches
/// the DB or the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Infohash(String);

impl Infohash {
    pub fn parse(raw: &str) -> Result<Self, String> {
        if iris_core::ids::is_infohash_hex(raw) {
            Ok(Self(raw.to_ascii_lowercase()))
        } else {
            Err(format!(
                "invalid infohash `{raw}`: expected 40 hex characters"
            ))
        }
    }

    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl std::ops::Deref for Infohash {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Infohash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for Infohash {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::{Infohash, Path};
    use tower::ServiceExt;

    #[tokio::test]
    async fn a_malformed_segment_answers_the_json_bad_request() {
        async fn handler(Path(h): Path<Infohash>) -> String {
            h.into_inner()
        }
        let app = axum::Router::new().route("/t/{infohash}", axum::routing::get(handler));
        let bad = app
            .clone()
            .oneshot(
                axum::http::Request::get("/t/nope")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bad.status(), axum::http::StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(bad.into_body(), 1 << 16)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "bad_request");
        assert!(
            json["message"]
                .as_str()
                .unwrap()
                .contains("expected 40 hex characters")
        );

        let ok = app
            .oneshot(
                axum::http::Request::get("/t/98259BA623EEC5F33167C083B51B30122C7FA068")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ok.status(), axum::http::StatusCode::OK);
    }

    #[test]
    fn infohash_is_validated_and_lowercased() {
        let ok = Infohash::parse("ABCDEF0123456789abcdef0123456789ABCDEF01").unwrap();
        assert_eq!(&*ok, "abcdef0123456789abcdef0123456789abcdef01");
        assert!(Infohash::parse("abc").is_err());
        assert!(Infohash::parse("zz25 9ba623eec5f33167c083b51b30122c7fa06").is_err());
        assert!(Infohash::parse(&"a".repeat(64)).is_err());
        let de: Infohash =
            serde_json::from_str("\"98259BA623EEC5F33167C083B51B30122C7FA068\"").unwrap();
        assert_eq!(&*de, "98259ba623eec5f33167c083b51b30122c7fa068");
        assert!(serde_json::from_str::<Infohash>("\"not-a-hash\"").is_err());
    }
}
