//! DNS-over-HTTPS resolver for the live TV clients.
//!
//! French ISPs DNS-block `vavoo.to` (it resolves to `127.0.0.1`; verified
//! 2026-10-03), which silently kills every Vavoo source on a server hosted
//! behind such a resolver. Resolving over HTTPS against IP-addressed public
//! resolvers sidesteps that without touching the host's DNS config:
//!
//! 1. Cloudflare `https://1.1.1.1/dns-query` (JSON API)
//! 2. Google `https://8.8.8.8/resolve`
//! 3. the system resolver, as a last resort
//!
//! Endpoints are IP literals, so the `DoH` client itself never needs DNS. Answers
//! are cached for their TTL (clamped to 1 min–1 h); a stale entry is still
//! served when every resolver fails, as it beats nothing. Ported from `SergeTV`.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use serde::Deserialize;

const ENDPOINTS: &[&str] = &["https://1.1.1.1/dns-query", "https://8.8.8.8/resolve"];
const QUERY_TIMEOUT: Duration = Duration::from_secs(4);
const MIN_TTL: u64 = 60;
const MAX_TTL: u64 = 3600;
/// Hosts past this many are swept of the entries stale for over
/// [`STALE_FOR`].
const CACHE_MAX: usize = 4096;
const STALE_FOR: Duration = Duration::from_hours(24);

#[derive(Deserialize)]
struct DohResponse {
    #[serde(rename = "Status")]
    status: u32,
    #[serde(rename = "Answer", default)]
    answer: Vec<DohAnswer>,
}

#[derive(Deserialize)]
struct DohAnswer {
    #[serde(rename = "type")]
    kind: u16,
    #[serde(rename = "TTL", default)]
    ttl: u64,
    data: String,
}

#[derive(Clone)]
struct CacheEntry {
    addrs: Vec<IpAddr>,
    expires: Instant,
}

#[derive(Clone)]
pub struct DohResolver {
    inner: Arc<Inner>,
}

struct Inner {
    http: reqwest::Client,
    /// [`ENDPOINTS`], or a test's fakes.
    endpoints: Vec<String>,
    cache: Mutex<HashMap<String, CacheEntry>>,
}

impl DohResolver {
    pub fn new() -> anyhow::Result<Self> {
        Self::with_endpoints(ENDPOINTS.iter().map(|e| (*e).to_string()).collect())
    }

    fn with_endpoints(endpoints: Vec<String>) -> anyhow::Result<Self> {
        let http = iris_providers::tls::client_builder()
            .timeout(QUERY_TIMEOUT)
            .connect_timeout(Duration::from_secs(3))
            .no_proxy()
            .build()?;
        Ok(Self {
            inner: Arc::new(Inner {
                http,
                endpoints,
                cache: Mutex::new(HashMap::new()),
            }),
        })
    }

    pub async fn lookup(&self, host: &str) -> std::io::Result<Vec<IpAddr>> {
        let key = host.to_ascii_lowercase();
        let cached = self
            .inner
            .cache
            .lock()
            .expect("poisoned")
            .get(&key)
            .cloned();
        if let Some(entry) = &cached
            && entry.expires > Instant::now()
        {
            return Ok(entry.addrs.clone());
        }

        // IPv4 first: containers commonly lack IPv6 egress, and a v6-only
        // answer would make every connect fail.
        let mut answered = 0;
        for kind in [1u16, 28] {
            for endpoint in &self.inner.endpoints {
                match self.query(endpoint, &key, kind).await {
                    Ok(Some((addrs, ttl))) => {
                        self.remember(&key, &addrs, ttl);
                        return Ok(addrs);
                    }
                    // Authoritative "no such record": try the next type, not
                    // the next resolver.
                    Ok(None) => {
                        answered += 1;
                        break;
                    }
                    Err(e) => {
                        tracing::debug!(endpoint, host = %key, error = %e, "doh query failed");
                    }
                }
            }
        }

        if let Some(entry) = cached {
            tracing::warn!(host = %key, "doh unavailable, serving stale dns entry");
            return Ok(entry.addrs);
        }
        // Both types answered "no such record": a dead feed's host, or a
        // name only the local resolver knows. Not a DoH outage.
        if answered == 2 {
            tracing::debug!(host = %key, "no public dns record, asking the system resolver");
        } else {
            tracing::warn!(host = %key, "doh unavailable, falling back to system dns");
        }
        let addrs: Vec<IpAddr> = tokio::net::lookup_host((key.as_str(), 0))
            .await?
            .map(|sa| sa.ip())
            .collect();
        Ok(addrs)
    }

    fn remember(&self, key: &str, addrs: &[IpAddr], ttl: u64) {
        let mut cache = self.inner.cache.lock().expect("poisoned");
        if cache.len() >= CACHE_MAX {
            // Kept stale for a DoH outage, but not forever: one entry per
            // host ever seen (CDN edges, logo hosts).
            let horizon = Instant::now().checked_sub(STALE_FOR);
            cache.retain(|_, e| horizon.is_none_or(|h| e.expires > h));
        }
        cache.insert(
            key.to_string(),
            CacheEntry {
                addrs: addrs.to_vec(),
                expires: Instant::now() + Duration::from_secs(ttl),
            },
        );
    }

    /// `Ok(None)` = the resolver answered but has no record of that type.
    async fn query(
        &self,
        endpoint: &str,
        host: &str,
        kind: u16,
    ) -> Result<Option<(Vec<IpAddr>, u64)>, reqwest::Error> {
        let resp: DohResponse = self
            .inner
            .http
            .get(endpoint)
            .query(&[
                ("name", host),
                ("type", if kind == 1 { "A" } else { "AAAA" }),
            ])
            .header(reqwest::header::ACCEPT, "application/dns-json")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if resp.status != 0 {
            return Ok(None);
        }
        let mut ttl = MAX_TTL;
        let addrs: Vec<IpAddr> = resp
            .answer
            .iter()
            .filter(|a| a.kind == kind)
            .filter_map(|a| {
                ttl = ttl.min(a.ttl);
                a.data.parse().ok()
            })
            // A resolver that "answers" loopback/unspecified is a blocking
            // resolver in disguise: never trust it.
            .filter(|ip: &IpAddr| !ip.is_loopback() && !ip.is_unspecified())
            .collect();
        if addrs.is_empty() {
            return Ok(None);
        }
        Ok(Some((addrs, ttl.clamp(MIN_TTL, MAX_TTL))))
    }
}

impl Resolve for DohResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let this = self.clone();
        Box::pin(async move {
            let ips = this.lookup(name.as_str()).await?;
            let addrs: Addrs = Box::new(ips.into_iter().map(|ip| SocketAddr::new(ip, 0)));
            Ok(addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use axum::extract::{Query, State};
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Response};
    use axum::routing::get;

    use super::*;

    #[derive(Default)]
    struct Fake {
        queries: AtomicUsize,
        down: AtomicBool,
    }

    /// A `DoH` JSON resolver: `v4.test` (two A records behind a CNAME and the
    /// loopback a blocking resolver answers), `v6.test` (AAAA only),
    /// `long.test` / `short.test` (TTLs out of range), the rest NXDOMAIN.
    async fn fake_doh(
        State(fake): State<Arc<Fake>>,
        Query(q): Query<HashMap<String, String>>,
    ) -> Response {
        fake.queries.fetch_add(1, Ordering::SeqCst);
        if fake.down.load(Ordering::SeqCst) {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        let a = |data: &str, ttl: u64| serde_json::json!({"type": 1, "TTL": ttl, "data": data});
        let answer = match (q["name"].as_str(), q["type"].as_str()) {
            ("v4.test", "A") => vec![
                serde_json::json!({"type": 5, "TTL": 300, "data": "edge.test."}),
                a("93.184.216.34", 300),
                a("93.184.216.35", 120),
                a("127.0.0.1", 300),
                a("0.0.0.0", 300),
            ],
            ("v6.test", "AAAA") => {
                vec![serde_json::json!({"type": 28, "TTL": 300, "data": "2606:4700::1111"})]
            }
            ("long.test", "A") => vec![a("93.184.216.36", 86_400)],
            ("short.test", "A") => vec![a("93.184.216.37", 5)],
            _ => return axum::Json(serde_json::json!({"Status": 3})).into_response(),
        };
        axum::Json(serde_json::json!({"Status": 0, "Answer": answer})).into_response()
    }

    /// A resolver asking the fake, behind `dead` closed endpoints.
    async fn fake(dead: usize) -> (DohResolver, Arc<Fake>) {
        let state = Arc::new(Fake::default());
        let app = axum::Router::new()
            .route("/dns-query", get(fake_doh))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut endpoints = Vec::new();
        for _ in 0..dead {
            let closed = std::net::TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap();
            endpoints.push(format!("http://{closed}/dns-query"));
        }
        endpoints.push(format!("http://{addr}/dns-query"));
        (DohResolver::with_endpoints(endpoints).unwrap(), state)
    }

    fn queries(fake: &Fake) -> usize {
        fake.queries.load(Ordering::SeqCst)
    }

    /// Seconds an answer stays cached.
    fn cached_for(r: &DohResolver, host: &str) -> u64 {
        let entry = r.inner.cache.lock().unwrap().get(host).cloned().unwrap();
        entry
            .expires
            .saturating_duration_since(Instant::now())
            .as_secs()
    }

    fn ips(list: &[&str]) -> Vec<IpAddr> {
        list.iter().map(|ip| ip.parse().unwrap()).collect()
    }

    #[tokio::test]
    async fn answers_are_screened_and_cached_for_their_shortest_ttl() {
        let (r, fake) = fake(0).await;
        let want = ips(&["93.184.216.34", "93.184.216.35"]);
        assert_eq!(
            r.lookup("V4.Test").await.unwrap(),
            want,
            "no CNAME, loopback or unspecified answer"
        );
        assert_eq!(r.lookup("v4.test").await.unwrap(), want);
        assert_eq!(queries(&fake), 1, "the second one cached, case aside");
        assert!((118..=120).contains(&cached_for(&r, "v4.test")));
    }

    #[tokio::test]
    async fn ttls_are_clamped() {
        let (r, _) = fake(0).await;
        r.lookup("long.test").await.unwrap();
        r.lookup("short.test").await.unwrap();
        assert!((MAX_TTL - 2..=MAX_TTL).contains(&cached_for(&r, "long.test")));
        assert!((MIN_TTL - 2..=MIN_TTL).contains(&cached_for(&r, "short.test")));
    }

    /// No A record: AAAA next, asked of the first resolver that answers.
    #[tokio::test]
    async fn ipv6_only_names_and_a_dead_resolver() {
        let (r, fake) = fake(1).await;
        assert_eq!(
            r.lookup("v6.test").await.unwrap(),
            ips(&["2606:4700::1111"])
        );
        assert_eq!(queries(&fake), 2, "one A, one AAAA");
    }

    #[tokio::test]
    async fn an_expired_answer_is_asked_again_and_served_stale_when_doh_is_down() {
        let (r, fake) = fake(0).await;
        let expired = CacheEntry {
            addrs: ips(&["198.51.100.7"]),
            expires: Instant::now().checked_sub(Duration::from_secs(1)).unwrap(),
        };
        r.inner
            .cache
            .lock()
            .unwrap()
            .insert("v4.test".into(), expired.clone());
        r.inner
            .cache
            .lock()
            .unwrap()
            .insert("gone.test".into(), expired);
        assert_eq!(r.lookup("v4.test").await.unwrap().len(), 2, "asked again");
        fake.down.store(true, Ordering::SeqCst);
        assert_eq!(
            r.lookup("gone.test").await.unwrap(),
            ips(&["198.51.100.7"]),
            "stale beats nothing"
        );
    }

    /// Nothing from `DoH` and nothing cached: the system resolver.
    #[tokio::test]
    async fn the_system_resolver_is_the_last_resort() {
        let (r, fake) = fake(0).await;
        let loopback =
            |addrs: Vec<IpAddr>| !addrs.is_empty() && addrs.iter().all(IpAddr::is_loopback);
        assert!(
            loopback(r.lookup("localhost").await.unwrap()),
            "no such name over DoH"
        );
        assert_eq!(queries(&fake), 2, "A, then AAAA");
        fake.down.store(true, Ordering::SeqCst);
        assert!(loopback(r.lookup("localhost").await.unwrap()), "DoH down");
    }

    /// Live: hits Cloudflare/Google. `cargo test -p iris-api doh_live -- --ignored`.
    #[tokio::test]
    #[ignore = "network"]
    async fn doh_live_resolves_vavoo() {
        let r = DohResolver::new().unwrap();
        let ips = r.lookup("www.vavoo.to").await.unwrap();
        assert_ne!(ips.len(), 0);
        assert!(ips.iter().all(|ip| !ip.is_loopback()), "{ips:?}");
    }
}
