//! `X-Iris-Client` header parsing + minimum-version gate.
//!
//! Every Iris client (Android TV APK, Web bundle) sends a single header
//! per request:
//!
//! ```text
//! X-Iris-Client: tv/0.2.0
//! X-Iris-Client: web/0.2.0-rc1
//! ```
//!
//! The middleware:
//! 1. **Always logs** the parsed `(kind, version)` for telemetry —
//!    useful for "what's our APK install base?" without DB writes.
//! 2. **Gates on minimum versions** ([`MIN_TV_VERSION`] /
//!    [`MIN_WEB_VERSION`]): when the client's version is strictly
//!    below, the request is short-circuited with a `426 Upgrade
//!    Required` and a machine-readable body so the client can show
//!    "Please update Iris in app settings" rather than a generic
//!    failure.
//!
//! Legacy clients that don't send the header at all (e.g. shipped
//! 0.1.0 APKs that pre-date this protocol) are **let through**:
//! we can't break them retroactively. Bump the `MIN_*_VERSION`
//! constants only AFTER the new client is widely installed AND the
//! in-app updater path is reachable without the gate (otherwise the
//! user is stuck on the lock-out screen with no way to update).

use std::str::FromStr;

use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use semver::Version;
use serde_json::json;

pub const CLIENT_HEADER: &str = "x-iris-client";

/// Minimum supported Android TV APK. Below this we return 426. Bump
/// only when a **breaking** server-side change shipped and clients
/// must update. Start permissive — every existing 0.1.0 APK in the
/// wild must keep working.
const MIN_TV_VERSION: &str = "0.0.0";
/// Minimum supported Web bundle. Same discipline as TV; the cached
/// bundle on a user's browser can be just as stale as an APK.
const MIN_WEB_VERSION: &str = "0.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientKind {
    Tv,
    Web,
}

impl ClientKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tv => "tv",
            Self::Web => "web",
        }
    }

    fn min_version(self) -> &'static str {
        match self {
            Self::Tv => MIN_TV_VERSION,
            Self::Web => MIN_WEB_VERSION,
        }
    }
}

impl FromStr for ClientKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            // Tolerate the longer `android-tv` form in case a downstream
            // build switches on the same convention.
            "tv" | "android-tv" => Ok(Self::Tv),
            "web" => Ok(Self::Web),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClientVersion {
    pub kind: ClientKind,
    pub version: Version,
}

impl ClientVersion {
    /// Parse `tv/0.2.0` / `web/0.2.0-rc1` (case-insensitive on the
    /// kind, strict semver on the version). Returns `None` on any
    /// malformed input — the middleware logs and lets the request
    /// continue rather than 400-ing, because a bad header is more
    /// often a proxy munge than a malicious client.
    pub fn parse(raw: &str) -> Option<Self> {
        let (kind_str, ver_str) = raw.split_once('/')?;
        let kind = kind_str.parse::<ClientKind>().ok()?;
        let version = Version::parse(ver_str.trim()).ok()?;
        Some(Self { kind, version })
    }
}

/// Axum middleware. Parses the header, logs, gates on
/// [`ClientKind::min_version`], lets every other request through.
pub async fn client_version_layer(req: Request<Body>, next: Next) -> Response {
    let raw = req
        .headers()
        .get(CLIENT_HEADER)
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned);

    let parsed = raw.as_deref().and_then(ClientVersion::parse);
    if let (Some(raw), None) = (raw.as_deref(), parsed.as_ref()) {
        // Header present but unparseable — probably a proxy stripped
        // the `/` or a future client kind. Log once per request and
        // continue — better to serve a request than to brick a user
        // over a misshapen header.
        tracing::warn!(header = %raw, "X-Iris-Client malformed, ignoring");
    }

    if let Some(client) = parsed.as_ref() {
        tracing::debug!(
            kind = client.kind.as_str(),
            version = %client.version,
            path = %req.uri().path(),
            "client request",
        );
        let min = Version::parse(client.kind.min_version())
            .expect("MIN_*_VERSION constants are semver literals");
        if client.version < min {
            tracing::info!(
                kind = client.kind.as_str(),
                version = %client.version,
                min = %min,
                "rejecting outdated client",
            );
            return outdated_response(client.kind, &client.version, &min);
        }
    }

    next.run(req).await
}

fn outdated_response(kind: ClientKind, your: &Version, min: &Version) -> Response {
    let message = match kind {
        ClientKind::Tv => "Please update Iris from Settings → Update app.",
        ClientKind::Web => "Please reload the page to pick up the latest Iris.",
    };
    (
        StatusCode::UPGRADE_REQUIRED,
        axum::Json(json!({
            "error": "client_outdated",
            "message": message,
            "client_kind": kind.as_str(),
            "your_version": your.to_string(),
            "min_version": min.to_string(),
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical() {
        let v = ClientVersion::parse("tv/0.2.0").unwrap();
        assert_eq!(v.kind, ClientKind::Tv);
        assert_eq!(v.version, Version::parse("0.2.0").unwrap());
    }

    #[test]
    fn parses_web_with_prerelease() {
        let v = ClientVersion::parse("web/0.2.0-rc1").unwrap();
        assert_eq!(v.kind, ClientKind::Web);
        assert_eq!(v.version, Version::parse("0.2.0-rc1").unwrap());
    }

    #[test]
    fn accepts_alternate_tv_alias() {
        let v = ClientVersion::parse("android-tv/0.2.0").unwrap();
        assert_eq!(v.kind, ClientKind::Tv);
    }

    #[test]
    fn rejects_malformed() {
        assert!(ClientVersion::parse("").is_none());
        assert!(ClientVersion::parse("tv").is_none());
        assert!(ClientVersion::parse("tv/").is_none());
        assert!(ClientVersion::parse("/0.2.0").is_none());
        assert!(ClientVersion::parse("tv/notaversion").is_none());
        assert!(ClientVersion::parse("phone/0.2.0").is_none());
    }

    #[test]
    fn min_version_constants_parse() {
        // Guards against typos in MIN_*_VERSION — the middleware would
        // otherwise panic at first request.
        Version::parse(MIN_TV_VERSION).expect("MIN_TV_VERSION is semver");
        Version::parse(MIN_WEB_VERSION).expect("MIN_WEB_VERSION is semver");
    }
}
