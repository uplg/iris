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
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ConnectInfo;
use governor::middleware::NoOpMiddleware;
use http::HeaderName;
use tower_governor::GovernorError;
use tower_governor::governor::{GovernorConfig, GovernorConfigBuilder};
use tower_governor::key_extractor::KeyExtractor;

const CF_CONNECTING_IP: HeaderName = HeaderName::from_static("cf-connecting-ip");

/// A lane's budget: `requests` per `period` sustained, `burst` back to back.
#[derive(Clone, Copy, Debug)]
pub struct Quota {
    pub requests: u32,
    pub period: Duration,
    pub burst: u32,
}

impl Quota {
    /// Login / register / passkey ceremonies (Argon2 or passkey work).
    pub const LOGIN: Self = Self {
        requests: 5,
        period: Duration::from_secs(1),
        burst: 20,
    };
    /// Refresh / logout / device pairing + polling, for a whole household.
    pub const SESSION: Self = Self {
        requests: 20,
        period: Duration::from_secs(1),
        burst: 60,
    };

    /// `tower_governor`'s `per_second(n)` is the interval between two refills
    /// (one token every n seconds), not n per second: the lane is expressed
    /// as that interval.
    pub const fn refill_interval(self) -> Duration {
        self.period
            .checked_div(self.requests)
            .expect("quota requests is non-zero")
    }
}

pub type LaneConfig = GovernorConfig<CloudflareIpKeyExtractor, NoOpMiddleware>;

pub fn lane(quota: Quota) -> Arc<LaneConfig> {
    Arc::new(
        GovernorConfigBuilder::default()
            .period(quota.refill_interval())
            .burst_size(quota.burst)
            .key_extractor(CloudflareIpKeyExtractor)
            .finish()
            .expect("lane quotas are non-zero constants"),
    )
}

/// Drop the buckets of clients idle long enough to be full again; without it
/// every address ever seen keeps an entry for the life of the process.
pub fn spawn_sweeper(lanes: Vec<Arc<LaneConfig>>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(600));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            for lane in &lanes {
                lane.limiter().retain_recent();
                lane.limiter().shrink_to_fit();
            }
        }
    });
}

#[derive(Clone, Debug)]
pub struct CloudflareIpKeyExtractor;

impl KeyExtractor for CloudflareIpKeyExtractor {
    type Key = IpAddr;

    fn extract<T>(&self, req: &http::Request<T>) -> Result<Self::Key, GovernorError> {
        Ok(client_ip(req.headers(), req.extensions()))
    }
}

/// The client a request comes from, as the module docs describe: the
/// `CF-Connecting-IP` set by the tunnel, else the peer socket, else loopback.
pub fn client_ip(headers: &http::HeaderMap, extensions: &http::Extensions) -> IpAddr {
    if let Some(hdr) = headers.get(&CF_CONNECTING_IP)
        && let Ok(s) = hdr.to_str()
        && let Ok(ip) = s.trim().parse::<IpAddr>()
    {
        return ip;
    }
    // No CF header => the request didn't come through the tunnel. Key on
    // the real peer socket IP so each LAN device gets its own bucket
    // (see module docs). Tunnelled traffic always carries the CF header
    // and is handled above; everything reaching here is a direct peer.
    if let Some(ConnectInfo(addr)) = extensions.get::<ConnectInfo<SocketAddr>>() {
        return addr.ip();
    }
    // Connect-info absent (server not serving with connect-info) — collapse
    // onto one bucket rather than handing out unlimited fresh quota.
    IpAddr::V4(Ipv4Addr::LOCALHOST)
}

/// [`client_ip`] as a handler argument.
pub struct ClientIp(pub IpAddr);

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for ClientIp {
    type Rejection = std::convert::Infallible;

    fn from_request_parts(
        parts: &mut http::request::Parts,
        _state: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        std::future::ready(Ok(Self(client_ip(&parts.headers, &parts.extensions))))
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::Duration;

    use super::{Quota, lane};

    fn household() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7))
    }

    #[test]
    fn quotas_are_per_second_rates() {
        assert_eq!(Quota::LOGIN.refill_interval(), Duration::from_millis(200));
        assert_eq!(Quota::SESSION.refill_interval(), Duration::from_millis(50));
    }

    #[test]
    fn a_lane_allows_its_burst_then_refills_at_its_rate() {
        for quota in [Quota::LOGIN, Quota::SESSION] {
            let config = lane(quota);
            let limiter = config.limiter();
            for i in 0..quota.burst {
                assert!(limiter.check_key(&household()).is_ok(), "request {i}");
            }
            assert!(limiter.check_key(&household()).is_err());
            std::thread::sleep(quota.refill_interval() * 2);
            assert!(limiter.check_key(&household()).is_ok());
        }
    }
}
