//! Shared TLS root configuration for every outbound HTTP client.
//!
//! reqwest's `rustls` feature verifies server certs against the OS trust store
//! via `rustls-platform-verifier`. In the Chainguard/Wolfi runtime image that
//! store is incomplete/stale, so perfectly valid certs (Wikimedia, TMDB, …)
//! fail verification as `UnknownIssuer` or `certificate expired` (a stale
//! intermediate in the chain) — breaking logos, EPG, TMDB metadata, provider
//! searches on the deployed server while working fine in dev.
//!
//! Instead we pin **Mozilla's CA bundle into the binary** (`webpki-root-certs`)
//! and hand it to reqwest via [`ClientBuilder::tls_certs_only`], which switches
//! reqwest to a webpki verifier over exactly these roots and drops the
//! platform verifier — so trust no longer depends on the container's
//! `/etc/ssl/certs`. Every internet host we talk to chains to a public Mozilla
//! root, so nothing is lost.

/// Mozilla's CA roots as reqwest certificates. Parsed fresh per call (a
/// handful of clients at startup, ~150 certs each — negligible one-off cost).
pub fn webpki_roots() -> Vec<reqwest::Certificate> {
    webpki_root_certs::TLS_SERVER_ROOT_CERTS
        .iter()
        .filter_map(|der| reqwest::Certificate::from_der(der.as_ref()).ok())
        .collect()
}

/// A [`reqwest::ClientBuilder`] whose TLS trust is the bundled Mozilla roots
/// (image-independent). Callers still add their own user-agent, timeouts,
/// redirect policy, etc. Use this for EVERY strict outbound client; the only
/// exception is the Live TV logo fetcher, which deliberately accepts invalid
/// certs (cosmetic third-party hosts).
pub fn client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder().tls_certs_only(webpki_roots())
}
