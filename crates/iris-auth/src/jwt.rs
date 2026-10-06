use chrono::{DateTime, Duration, Utc};
use iris_core::ids::UserId;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum JwtError {
    #[error("jwt error: {0}")]
    Encode(#[from] jsonwebtoken::errors::Error),
    #[error("token expired")]
    Expired,
    #[error("invalid token kind: expected {expected:?}, got {got:?}")]
    WrongKind { expected: TokenKind, got: TokenKind },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenKind {
    Access,
    Refresh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessClaims {
    pub sub: Uuid,
    pub kind: TokenKind,
    pub admin: bool,
    pub exp: i64,
    pub iat: i64,
    pub iss: String,
    /// `iat` in milliseconds: a password change cuts sessions at an instant
    /// finer than `iat`'s seconds, so a sign-in in the same second as the cut
    /// survives it. Absent from the tokens minted before it existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iat_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshClaims {
    pub sub: Uuid,
    pub kind: TokenKind,
    pub jti: Uuid,
    pub exp: i64,
    pub iat: i64,
    pub iss: String,
    /// `iat` in milliseconds: a password change cuts sessions at an instant
    /// finer than `iat`'s seconds, so a sign-in in the same second as the cut
    /// survives it. Absent from the tokens minted before it existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iat_ms: Option<i64>,
}

impl AccessClaims {
    /// When the token was minted, in milliseconds; a token without `iat_ms`
    /// counts as minted at the start of its `iat` second, the earliest it can be.
    #[must_use]
    pub fn issued_at_ms(&self) -> i64 {
        self.iat_ms.unwrap_or(self.iat.saturating_mul(1000))
    }
}

impl RefreshClaims {
    /// See [`AccessClaims::issued_at_ms`].
    #[must_use]
    pub fn issued_at_ms(&self) -> i64 {
        self.iat_ms.unwrap_or(self.iat.saturating_mul(1000))
    }
}

/// A refresh token as minted: the JWT, its id, and the instants it carries
/// (stored as is, so [`Issuer::encode_refresh`] can mint the same JWT again).
#[derive(Debug, Clone)]
pub struct IssuedRefresh {
    pub token: String,
    pub jti: Uuid,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct Issuer {
    pub issuer: String,
    pub access_ttl: Duration,
    pub refresh_ttl: Duration,
    encoding: EncodingKey,
    decoding: DecodingKey,
}

impl Issuer {
    pub fn new(secret: &str, issuer: String, access_ttl_secs: i64, refresh_ttl_secs: i64) -> Self {
        Self {
            issuer,
            access_ttl: Duration::seconds(access_ttl_secs),
            refresh_ttl: Duration::seconds(refresh_ttl_secs),
            encoding: EncodingKey::from_secret(secret.as_bytes()),
            decoding: DecodingKey::from_secret(secret.as_bytes()),
        }
    }

    pub fn issue_access(&self, user_id: UserId, is_admin: bool) -> Result<String, JwtError> {
        let now = Utc::now();
        let claims = AccessClaims {
            sub: user_id.into(),
            kind: TokenKind::Access,
            admin: is_admin,
            iat: now.timestamp(),
            exp: (now + self.access_ttl).timestamp(),
            iss: self.issuer.clone(),
            iat_ms: Some(now.timestamp_millis()),
        };
        Ok(encode(&Header::default(), &claims, &self.encoding)?)
    }

    /// `ttl_override` replaces the configured refresh TTL for this token
    /// (device-paired sessions get a much longer window). It MUST be encoded
    /// into the JWT's own `exp`, not just tracked DB-side: `verify_refresh`
    /// validates `exp` before any DB lookup, so a JWT carrying the short
    /// default would 401 an otherwise-valid device session once the default
    /// window elapsed — the "rarely-used TV logs out after a week" bug.
    pub fn issue_refresh(
        &self,
        user_id: UserId,
        ttl_override: Option<Duration>,
    ) -> Result<IssuedRefresh, JwtError> {
        let issued_at = Utc::now();
        let jti = Uuid::new_v4();
        let expires_at = issued_at + ttl_override.unwrap_or(self.refresh_ttl);
        let token = self.encode_refresh(user_id, jti, issued_at, expires_at)?;
        Ok(IssuedRefresh {
            token,
            jti,
            issued_at,
            expires_at,
        })
    }

    /// The refresh JWT for these claims. Deterministic (HMAC), so a session
    /// re-encoded from its stored row is the very token first handed out.
    pub fn encode_refresh(
        &self,
        user_id: UserId,
        jti: Uuid,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<String, JwtError> {
        let claims = RefreshClaims {
            sub: user_id.into(),
            kind: TokenKind::Refresh,
            jti,
            iat: issued_at.timestamp(),
            exp: expires_at.timestamp(),
            iss: self.issuer.clone(),
            iat_ms: Some(issued_at.timestamp_millis()),
        };
        Ok(encode(&Header::default(), &claims, &self.encoding)?)
    }

    pub fn verify_access(&self, token: &str) -> Result<AccessClaims, JwtError> {
        let mut validation = Validation::default();
        validation.set_issuer(&[&self.issuer]);
        let data = decode::<AccessClaims>(token, &self.decoding, &validation)?;
        if data.claims.kind != TokenKind::Access {
            return Err(JwtError::WrongKind {
                expected: TokenKind::Access,
                got: data.claims.kind,
            });
        }
        Ok(data.claims)
    }

    pub fn verify_refresh(&self, token: &str) -> Result<RefreshClaims, JwtError> {
        let mut validation = Validation::default();
        validation.set_issuer(&[&self.issuer]);
        let data = decode::<RefreshClaims>(token, &self.decoding, &validation)?;
        if data.claims.kind != TokenKind::Refresh {
            return Err(JwtError::WrongKind {
                expected: TokenKind::Refresh,
                got: data.claims.kind,
            });
        }
        Ok(data.claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issuer() -> Issuer {
        Issuer::new("secret", "iris-test".into(), 3600, 7 * 24 * 3600)
    }

    /// Device sessions pass a TTL override; the JWT's own `exp` must carry
    /// it. If only the DB row got the long TTL (the old behavior), the token
    /// would fail `verify_refresh`'s exp validation after the short default
    /// window even though the session was still valid DB-side — logging out
    /// any device idle longer than the browser TTL.
    #[test]
    fn refresh_ttl_override_reaches_jwt_exp() {
        let iss = issuer();
        let year = Duration::days(365);
        let before = Utc::now();
        let IssuedRefresh {
            token,
            expires_at: exp,
            ..
        } = iss
            .issue_refresh(UserId::from(Uuid::new_v4()), Some(year))
            .expect("issue");
        assert!(
            exp >= before + year,
            "returned exp must honour the override"
        );
        let claims = iss.verify_refresh(&token).expect("verify");
        assert!(
            claims.exp >= (before + year - Duration::minutes(1)).timestamp(),
            "JWT-encoded exp must honour the override, got {} (≈{} days out)",
            claims.exp,
            (claims.exp - before.timestamp()) / 86_400,
        );
    }

    #[test]
    fn refresh_without_override_uses_configured_ttl() {
        let iss = issuer();
        let before = Utc::now();
        let IssuedRefresh {
            token,
            expires_at: exp,
            ..
        } = iss
            .issue_refresh(UserId::from(Uuid::new_v4()), None)
            .expect("issue");
        let claims = iss.verify_refresh(&token).expect("verify");
        let ceiling = (before + Duration::days(8)).timestamp();
        assert!(claims.exp <= ceiling, "default TTL must stay ~7 days");
        assert_eq!(claims.exp, exp.timestamp());
    }

    #[test]
    fn a_refresh_token_re_encodes_to_the_same_jwt() {
        let iss = issuer();
        let user = UserId::from(Uuid::new_v4());
        let first = iss.issue_refresh(user, None).expect("issue");
        let again = iss
            .encode_refresh(user, first.jti, first.issued_at, first.expires_at)
            .expect("encode");
        assert_eq!(first.token, again);
        let claims = iss.verify_refresh(&again).expect("verify");
        assert_eq!(claims.issued_at_ms(), first.issued_at.timestamp_millis());
    }

    #[test]
    fn a_token_without_iat_ms_counts_from_its_iat_second() {
        let claims: AccessClaims = serde_json::from_value(serde_json::json!({
            "sub": Uuid::nil(), "kind": "access", "admin": false,
            "exp": 2, "iat": 1, "iss": "x",
        }))
        .expect("an old token still parses");
        assert_eq!(claims.issued_at_ms(), 1000);
    }
}
