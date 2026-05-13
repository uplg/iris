//! HTTP middlewares shared across the API.
//!
//! - [`iris_caps_layer`] parses the `Iris-Caps` request header into a typed
//!   [`iris_caps::ClientCapabilities`] and attaches it to request extensions,
//!   so handlers can read it via [`IrisCaps`]. It also fires a best-effort
//!   background INSERT into `playback_caps_log` for telemetry.
//! - [`coop_coep_layers`] sets the `Cross-Origin-Opener-Policy` and
//!   `Cross-Origin-Embedder-Policy` headers required for `SharedArrayBuffer`
//!   in the web client (needed by `libav.js` threads + `SubtitlesOctopus`).

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderName, HeaderValue, header};
use axum::middleware::Next;
use axum::response::Response;
use iris_caps::ClientCapabilities;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::state::AppState;

pub const IRIS_CAPS_HEADER: &str = "iris-caps";

/// Wrapper attached to request extensions by [`iris_caps_layer`]. Handlers
/// can pull it out via [`axum::Extension`].
#[derive(Debug, Clone)]
pub struct IrisCaps(pub ClientCapabilities);

/// Middleware that parses the `Iris-Caps` header, attaches a typed
/// [`IrisCaps`] to the request, and fires a fire-and-forget INSERT into
/// `playback_caps_log`. Always continues the chain; never fails the request.
pub async fn iris_caps_layer(
    State(state): State<AppState>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    let header_value = req
        .headers()
        .get(IRIS_CAPS_HEADER)
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned);
    if let Some(raw) = header_value {
        let caps = ClientCapabilities::parse(&raw);
        let user_agent = req
            .headers()
            .get(header::USER_AGENT)
            .and_then(|h| h.to_str().ok())
            .map(str::to_owned);
        let path = req.uri().path().to_owned();
        let pool = state.db().clone();
        let caps_for_log = caps.clone();
        tokio::spawn(async move {
            log_caps(&pool, &path, &caps_for_log, user_agent.as_deref()).await;
        });
        req.extensions_mut().insert(IrisCaps(caps));
    }
    next.run(req).await
}

async fn log_caps(
    pool: &sqlx::SqlitePool,
    path: &str,
    caps: &ClientCapabilities,
    user_agent: Option<&str>,
) {
    let Ok(caps_json) = serde_json::to_string(caps) else {
        return;
    };
    let (infohash, file_idx, route) = parse_torrent_path(path);
    if let Err(e) = iris_db::playback_caps::insert(
        pool,
        infohash.as_deref(),
        file_idx,
        route.as_deref(),
        &caps_json,
        user_agent,
        None,
    )
    .await
    {
        tracing::debug!(error = %e, "playback_caps_log insert failed");
    }
}

/// Parse an api torrents path into `(infohash, file_idx, route_kind)`.
///
/// `/api/torrents/<hash>/files/<idx>/manifest.json` → `(Some(hash), Some(idx), Some("manifest.json"))`
/// Anything else → leading components present, trailing `None`.
fn parse_torrent_path(path: &str) -> (Option<String>, Option<i64>, Option<String>) {
    let mut segs = path.split('/').filter(|s| !s.is_empty());
    if segs.next() != Some("api") || segs.next() != Some("torrents") {
        return (None, None, None);
    }
    let infohash = segs.next().map(str::to_owned);
    if segs.next() != Some("files") {
        return (infohash, None, None);
    }
    let file_idx = segs.next().and_then(|s| s.parse::<i64>().ok());
    let route = segs.next().map(str::to_owned);
    (infohash, file_idx, route)
}

/// Tower-http layers that set `Cross-Origin-Opener-Policy: same-origin` and
/// `Cross-Origin-Embedder-Policy: credentialless` on every response.
///
/// `credentialless` is the looser variant of `require-corp`; it lets us
/// embed cross-origin assets (`TMDb` posters etc.) without forcing every
/// upstream CDN to send `Cross-Origin-Resource-Policy: cross-origin`. The
/// shaped pair satisfies the cross-origin-isolated check that
/// `SharedArrayBuffer` requires.
#[must_use]
pub fn coop_coep_layers() -> (
    SetResponseHeaderLayer<HeaderValue>,
    SetResponseHeaderLayer<HeaderValue>,
) {
    (
        SetResponseHeaderLayer::overriding(
            HeaderName::from_static("cross-origin-opener-policy"),
            HeaderValue::from_static("same-origin"),
        ),
        SetResponseHeaderLayer::overriding(
            HeaderName::from_static("cross-origin-embedder-policy"),
            HeaderValue::from_static("credentialless"),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::parse_torrent_path;

    #[test]
    fn parses_manifest_path() {
        let (h, i, r) = parse_torrent_path("/api/torrents/abc123/files/0/manifest.json");
        assert_eq!(h.as_deref(), Some("abc123"));
        assert_eq!(i, Some(0));
        assert_eq!(r.as_deref(), Some("manifest.json"));
    }

    #[test]
    fn parses_stream_path() {
        let (h, i, r) = parse_torrent_path("/api/torrents/deadbeef/files/2/stream");
        assert_eq!(h.as_deref(), Some("deadbeef"));
        assert_eq!(i, Some(2));
        assert_eq!(r.as_deref(), Some("stream"));
    }

    #[test]
    fn parses_torrent_only_path() {
        let (h, i, r) = parse_torrent_path("/api/torrents/foo");
        assert_eq!(h.as_deref(), Some("foo"));
        assert_eq!(i, None);
        assert_eq!(r, None);
    }

    #[test]
    fn rejects_unrelated_path() {
        let (h, i, r) = parse_torrent_path("/api/me/devices");
        assert_eq!(h, None);
        assert_eq!(i, None);
        assert_eq!(r, None);
    }
}
