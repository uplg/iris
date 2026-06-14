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
//! Requests arriving without `CF-Connecting-IP` (direct origin access
//! during dev, or anything that bypassed the tunnel) collapse onto a
//! single shared key (`127.0.0.1`). That keeps a header-strip bypass
//! from giving the attacker a fresh bucket per request.

use std::net::{IpAddr, Ipv4Addr};

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
        // No CF header => direct origin access (dev, or a bypass
        // attempt). Collapse all such callers onto one bucket so an
        // attacker can't strip the header and earn a fresh quota.
        Ok(IpAddr::V4(Ipv4Addr::LOCALHOST))
    }
}
