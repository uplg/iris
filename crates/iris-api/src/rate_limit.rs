//! Per-IP rate limiting for the auth surface, behind a Cloudflare tunnel.
//!
//! Wraps [`tower_governor`] with a Cloudflare-aware [`KeyExtractor`]:
//! the tunnel terminates locally (peer IP = loopback for every request),
//! so we cannot use `PeerIpKeyExtractor` / `SmartIpKeyExtractor`. Instead
//! we trust the `CF-Connecting-IP` header — Cloudflare sets it to the
//! real client IP on every request, and because the only path into the
//! origin is through the tunnel, an attacker cannot spoof it without
//! first bypassing Cloudflare.
//!
//! Normal traffic — including a LAN Android TV reaching Iris through the
//! Cloudflare URL — always carries `CF-Connecting-IP`, set to the client's
//! public IP. A household sits behind one NAT, so that is a single shared key
//! for every device in the home; the per-lane bucket sizing in `app.rs`
//! accounts for that aggregate.
//!
//! Requests arriving WITHOUT `CF-Connecting-IP` only happen on direct origin
//! access (dev, or something bypassing the tunnel). Those are keyed on the
//! real peer socket IP so each direct caller gets its own bucket instead of
//! all of them collapsing onto one shared key. A tunnelled header-strip
//! attempt still cannot earn fresh quota: its socket IP is the tunnel's
//! loopback, so all such requests share the loopback bucket. Connect-info is
//! absent only if the server is served without
//! `into_make_service_with_connect_info`; we fall back to loopback then.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::extract::ConnectInfo;
use http::HeaderName;
use tower_governor::GovernorError;
use tower_governor::key_extractor::KeyExtractor;

const CF_CONNECTING_IP: HeaderName = HeaderName::from_static("cf-connecting-ip");

#[derive(Clone, Debug)]
pub struct CloudflareIpKeyExtractor;

impl KeyExtractor for CloudflareIpKeyExtractor {
    type Key = IpAddr;

    fn extract<T>(&self, req: &http::Request<T>) -> Result<Self::Key, GovernorError> {
        if let Some(hdr) = req.headers().get(&CF_CONNECTING_IP)
            && let Ok(s) = hdr.to_str()
            && let Ok(ip) = s.trim().parse::<IpAddr>()
        {
            return Ok(ip);
        }
        // No CF header => the request didn't come through the tunnel. Key on
        // the real peer socket IP so each LAN device gets its own bucket
        // (see module docs). Tunnelled traffic always carries the CF header
        // and is handled above; everything reaching here is a direct peer.
        if let Some(ConnectInfo(addr)) = req.extensions().get::<ConnectInfo<SocketAddr>>() {
            return Ok(addr.ip());
        }
        // Connect-info absent (server not serving with connect-info) — collapse
        // onto one bucket rather than handing out unlimited fresh quota.
        Ok(IpAddr::V4(Ipv4Addr::LOCALHOST))
    }
}
