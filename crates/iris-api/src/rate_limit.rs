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

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
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
/// `CF-Connecting-IP` set by the tunnel, else the peer socket, else loopback;
/// folded by [`address_key`].
pub fn client_ip(headers: &http::HeaderMap, extensions: &http::Extensions) -> IpAddr {
    address_key(raw_client_ip(headers, extensions))
}

/// What counts as one client: an IPv4 address, or an IPv6 /64 (one
/// household's prefix). Keying on the full IPv6 address would hand a
/// client 2^64 fresh buckets.
fn address_key(ip: IpAddr) -> IpAddr {
    match ip.to_canonical() {
        IpAddr::V6(v6) => IpAddr::V6(Ipv6Addr::from_bits(v6.to_bits() & (u128::MAX << 64))),
        v4 @ IpAddr::V4(_) => v4,
    }
}

fn raw_client_ip(headers: &http::HeaderMap, extensions: &http::Extensions) -> IpAddr {
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

    use super::{Quota, client_ip, lane};

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

    #[test]
    fn an_ipv6_household_is_one_client() {
        let key = |ip: &str| {
            let mut headers = http::HeaderMap::new();
            headers.insert("cf-connecting-ip", ip.parse().unwrap());
            client_ip(&headers, &http::Extensions::new())
        };
        assert_eq!(key("2001:db8:1:2:aaaa::1"), key("2001:db8:1:2:bbbb::2"));
        assert_ne!(key("2001:db8:1:2::1"), key("2001:db8:1:3::1"));
        assert_eq!(
            key("::ffff:192.0.2.1"),
            "192.0.2.1".parse::<IpAddr>().unwrap()
        );
    }
}
