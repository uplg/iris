//! Vavoo (a.k.a. lokke) live-channel source: catalog fetch + on-demand
//! stream resolution.
//!
//! Vavoo aggregates clear-HLS restreams of European TNT networks behind a
//! signed `MediaHubMX` API — including the channels `iptv-org` and the official
//! CDNs no longer carry (the whole M6 group DMCAs its restreams, so M6 / W9 /
//! 6ter vanished from every static playlist). We surface them as extra
//! `Community`-tier sources: each catalog entry becomes an [`M3uEntry`] whose
//! URL is a `vavoo://<id>` sentinel, resolved to a fresh tokenised
//! `index.m3u8` only when a viewer actually tunes the channel — the token and
//! the edge host both rotate, so resolution is deferred to playback and never
//! cached for long.
//!
//! Two API hops: a signing `ping2` (fixed public vector → a short-lived
//! `mediahubmx-signature`) then a `mediahubmx-resolve`/`-catalog` call
//! carrying that signature. The signature is reused across calls until its
//! TTL lapses.

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::m3u::M3uEntry;

const PING_URL: &str = "https://www.vavoo.tv/api/box/ping2";
const RESOLVE_URL: &str = "https://www.vavoo.to/mediahubmx-resolve.json";
const CATALOG_URL: &str = "https://www.vavoo.to/mediahubmx-catalog.json";
const PLAY_PREFIX: &str = "https://vavoo.to/vavoo-iptv/play/";

/// Sentinel scheme a Vavoo channel source carries in place of a real URL;
/// [`resolve`](Vavoo::resolve) turns `vavoo://<id>` into the live playlist.
pub const SCHEME: &str = "vavoo://";
/// UA Vavoo's Android app presents to the signing endpoint.
const API_UA: &str = "okhttp/4.11.0";
/// UA for the `MediaHubMX` resolve/catalog calls.
const RESOLVE_UA: &str = "MediaHubMX/2";
/// UA + referer the resolved stream (master + segments) demands.
pub const STREAM_UA: &str = "VAVOO/2.6";
pub const STREAM_REFERER: &str = "https://vavoo.to/";

/// Fixed signing vector the `ping2` box endpoint accepts. Public and shared
/// by every Vavoo client — not a secret, just the handshake payload.
const VEC: &str = "9frjpxPjxSNilxJPCJ0XGYs6scej3dW/h/VWlnKUiLSG8IP7mfyDU7NirOlld+VtCKGj03XjetfliDMhIev7wcARo+YTU8KPFuVQP9E2DVXzY2BFo1NhE6qEmPfNDnm74eyl/7iFJ0EETm6XbYyz8IKBkAqPN/Spp3PZ2ulKg3QBSDxcVN4R5zRn7OsgLJ2CNTuWkd/h451lDCp+TtTuvnAEhcQckdsydFhTZCK5IiWrrTIC/d4qDXEd+GtOP4hPdoIuCaNzYfX3lLCwFENC6RZoTBYLrcKVVgbqyQZ7DnLqfLqvf3z0FVUWx9H21liGFpByzdnoxyFkue3NzrFtkRL37xkx9ITucepSYKzUVEfyBh+/3mtzKY26VIRkJFkpf8KVcCRNrTRQn47Wuq4gC7sSwT7eHCAydKSACcUMMdpPSvbvfOmIqeBNA83osX8FPFYUMZsjvYNEE3arbFiGsQlggBKgg1V3oN+5ni3Vjc5InHg/xv476LHDFnNdAJx448ph3DoAiJjr2g4ZTNynfSxdzA68qSuJY8UjyzgDjG0RIMv2h7DlQNjkAXv4k1BrPpfOiOqH67yIarNmkPIwrIV+W9TTV/yRyE1LEgOr4DK8uW2AUtHOPA2gn6P5sgFyi68w55MZBPepddfYTQ+E1N6R/hWnMYPt/i0xSUeMPekX47iucfpFBEv9Uh9zdGiEB+0P3LVMP+q+pbBU4o1NkKyY1V8wH1Wilr0a+q87kEnQ1LWYMMBhaP9yFseGSbYwdeLsX9uR1uPaN+u4woO2g8sw9Y5ze5XMgOVpFCZaut02I5k0U4WPyN5adQjG8sAzxsI3KsV04DEVymj224iqg2Lzz53Xz9yEy+7/85ILQpJ6llCyqpHLFyHq/kJxYPhDUF755WaHJEaFRPxUqbparNX+mCE9Xzy7Q/KTgAPiRS41FHXXv+7XSPp4cy9jli0BVnYf13Xsp28OGs/D8Nl3NgEn3/eUcMN80JRdsOrV62fnBVMBNf36+LbISdvsFAFr0xyuPGmlIETcFyxJkrGZnhHAxwzsvZ+Uwf8lffBfZFPRrNv+tgeeLpatVcHLHZGeTgWWml6tIHwWUqv2TVJeMkAEL5PPS4Gtbscau5HM+FEjtGS+KClfX1CNKvgYJl7mLDEf5ZYQv5kHaoQ6RcPaR6vUNn02zpq5/X3EPIgUKF0r/0ctmoT84B2J1BKfCbctdFY9br7JSJ6DvUxyde68jB+Il6qNcQwTFj4cNErk4x719Y42NoAnnQYC2/qfL/gAhJl8TKMvBt3Bno+va8ve8E0z8yEuMLUqe8OXLce6nCa+L5LYK1aBdb60BYbMeWk1qmG6Nk9OnYLhzDyrd9iHDd7X95OM6X5wiMVZRn5ebw4askTTc50xmrg4eic2U1w1JpSEjdH/u/hXrWKSMWAxaj34uQnMuWxPZEXoVxzGyuUbroXRfkhzpqmqqqOcypjsWPdq5BOUGL/Riwjm6yMI0x9kbO8+VoQ6RYfjAbxNriZ1cQ+AW1fqEgnRWXmjt4Z1M0ygUBi8w71bDML1YG6UHeC2cJ2CCCxSrfycKQhpSdI1QIuwd2eyIpd4LgwrMiY3xNWreAF+qobNxvE7ypKTISNrz0iYIhU0aKNlcGwYd0FXIRfKVBzSBe4MRK2pGLDNO6ytoHxvJweZ8h1XG8RWc4aB5gTnB7Tjiqym4b64lRdj1DPHJnzD4aqRixpXhzYzWVDN2kONCR5i2quYbnVFN4sSfLiKeOwKX4JdmzpYixNZXjLkG14seS6KR0Wl8Itp5IMIWFpnNokjRH76RYRZAcx0jP0V5/GfNNTi5QsEU98en0SiXHQGXnROiHpRUDXTl8FmJORjwXc0AjrEMuQ2FDJDmAIlKUSLhjbIiKw3iaqp5TVyXuz0ZMYBhnqhcwqULqtFSuIKpaW8FgF8QJfP2frADf4kKZG1bQ99MrRrb2A=";

/// How long a fetched signing signature stays reusable.
const SIGNATURE_TTL: Duration = Duration::from_mins(30);
/// How long a resolved playlist URL is reused (the token rotates fast — keep
/// this short so a stale URL self-heals on the next zap rather than 403ing).
const RESOLVE_TTL: Duration = Duration::from_secs(90);
/// Safety cap on catalog paging (300 items/page).
const CATALOG_MAX_PAGES: usize = 24;

/// `vavoo://<id>` → `Some("<id>")`, any other URL → `None`.
pub fn stream_id(url: &str) -> Option<&str> {
    url.strip_prefix(SCHEME)
}

#[derive(Deserialize)]
struct PingResponse {
    response: Option<PingInner>,
}
#[derive(Deserialize)]
struct PingInner {
    signed: Option<String>,
}

#[derive(Deserialize)]
struct ResolveItem {
    url: Option<String>,
}

#[derive(Deserialize)]
struct CatalogResponse {
    #[serde(default)]
    items: Vec<CatalogItem>,
    #[serde(rename = "nextCursor", default)]
    next_cursor: Option<i64>,
}
#[derive(Deserialize)]
struct CatalogItem {
    #[serde(default)]
    name: String,
    #[serde(default)]
    logo: Option<String>,
    #[serde(default)]
    ids: CatalogIds,
}
#[derive(Deserialize, Default)]
struct CatalogIds {
    #[serde(default)]
    id: Option<String>,
}

/// Vavoo API client + its short-lived caches. Holds no HTTP client of its own
/// — the live-tv service passes its shared [`reqwest::Client`] in, so Vavoo
/// inherits the same TLS / timeout / redirect knobs as every other upstream.
#[derive(Default)]
pub struct Vavoo {
    signature: RwLock<Option<(String, Instant)>>,
    resolved: RwLock<HashMap<String, (String, Instant)>>,
}

impl Vavoo {
    /// A reusable signing signature, fetched (and cached) on demand. `None`
    /// when the signing endpoint is unreachable — every caller degrades to
    /// "no Vavoo channels this round" rather than failing.
    async fn signature(&self, http: &reqwest::Client) -> Option<String> {
        if let Some((sig, at)) = self.signature.read().expect("poisoned").clone()
            && at.elapsed() < SIGNATURE_TTL
        {
            return Some(sig);
        }
        let body = serde_json::json!({ "vec": VEC });
        let resp = http
            .post(PING_URL)
            .header(reqwest::header::USER_AGENT, API_UA)
            .header(reqwest::header::ACCEPT, "application/json")
            .json(&body)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .ok()?;
        let parsed: PingResponse = resp.json().await.ok()?;
        let sig = parsed.response.and_then(|r| r.signed)?;
        *self.signature.write().expect("poisoned") = Some((sig.clone(), Instant::now()));
        Some(sig)
    }

    /// Resolve `vavoo://<id>` to a live `index.m3u8`. Cached for
    /// [`RESOLVE_TTL`] so the health path and the actual zap share one API
    /// call. `None` when signing or resolution fails.
    pub async fn resolve(&self, http: &reqwest::Client, id: &str) -> Option<String> {
        if let Some((url, at)) = self.resolved.read().expect("poisoned").get(id).cloned()
            && at.elapsed() < RESOLVE_TTL
        {
            return Some(url);
        }
        let sig = self.signature(http).await?;
        let body = serde_json::json!({
            "language": "de",
            "region": "AT",
            "url": format!("{PLAY_PREFIX}{id}"),
            "clientVersion": "3.0.2",
        });
        let resp = http
            .post(RESOLVE_URL)
            .header(reqwest::header::USER_AGENT, RESOLVE_UA)
            .header(reqwest::header::ACCEPT, "application/json")
            .header("mediahubmx-signature", sig)
            .json(&body)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .ok()?;
        let items: Vec<ResolveItem> = resp.json().await.ok()?;
        let url = items.into_iter().find_map(|i| i.url)?;
        self.resolved
            .write()
            .expect("poisoned")
            .insert(id.to_string(), (url.clone(), Instant::now()));
        Some(url)
    }

    /// Every playable channel of the given Vavoo groups, as playlist entries
    /// ready for [`build_channels`](super::channels::build_channels). Best
    /// effort: a failed signing or page fetch yields whatever was collected so
    /// far (possibly empty).
    pub async fn entries_for_groups(
        &self,
        http: &reqwest::Client,
        groups: &[String],
    ) -> Vec<M3uEntry> {
        let Some(sig) = self.signature(http).await else {
            return Vec::new();
        };
        let mut entries = Vec::new();
        for group in groups {
            self.collect_group(http, &sig, group, &mut entries).await;
        }
        entries
    }

    async fn collect_group(
        &self,
        http: &reqwest::Client,
        sig: &str,
        group: &str,
        out: &mut Vec<M3uEntry>,
    ) {
        let mut cursor: i64 = 0;
        for _ in 0..CATALOG_MAX_PAGES {
            let body = serde_json::json!({
                "language": "en",
                "region": "AT",
                "catalogId": "iptv",
                "id": "iptv",
                "adult": false,
                "search": "",
                "sort": "name",
                "filter": { "group": group },
                "cursor": cursor,
                "clientVersion": "3.0.2",
            });
            let page: Option<CatalogResponse> = async {
                http.post(CATALOG_URL)
                    .header(reqwest::header::USER_AGENT, RESOLVE_UA)
                    .header(reqwest::header::ACCEPT, "application/json")
                    .header("mediahubmx-signature", sig)
                    .json(&body)
                    .send()
                    .await
                    .and_then(reqwest::Response::error_for_status)
                    .ok()?
                    .json()
                    .await
                    .ok()
            }
            .await;
            let Some(page) = page else { return };
            if page.items.is_empty() {
                return;
            }
            for item in page.items {
                let Some(id) = item.ids.id.filter(|s| !s.is_empty()) else {
                    continue;
                };
                let name = clean_name(&item.name);
                if name.is_empty() {
                    continue;
                }
                let mut attrs = HashMap::new();
                attrs.insert("group-title".to_string(), group.to_string());
                if let Some(logo) = item.logo.filter(|s| !s.is_empty()) {
                    attrs.insert("tvg-logo".to_string(), logo);
                }
                attrs.insert("http-user-agent".to_string(), STREAM_UA.to_string());
                attrs.insert("http-referrer".to_string(), STREAM_REFERER.to_string());
                out.push(M3uEntry {
                    name,
                    attrs,
                    vlc_opts: HashMap::new(),
                    url: format!("{SCHEME}{id}"),
                });
            }
            match page.next_cursor {
                Some(next) if next != cursor => cursor = next,
                _ => return,
            }
        }
    }
}

/// Drop Vavoo's trailing ` .<tag>` provider marker (`"M6 .c"` → `"M6"`,
/// `"M6 FHD .b"` → `"M6 FHD"`) so plain-named feeds fold onto the matching
/// iptv-org channel (and, for FR, pick up their TNT number) while quality
/// variants stay distinct.
fn clean_name(raw: &str) -> String {
    let n = raw.trim();
    if let Some(pos) = n.rfind(" .") {
        let tag = &n[pos + 2..];
        if !tag.is_empty() && tag.chars().all(|c| c.is_ascii_alphanumeric()) {
            return n[..pos].trim().to_string();
        }
    }
    n.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_id_extracts_sentinel() {
        assert_eq!(stream_id("vavoo://abc123"), Some("abc123"));
        assert_eq!(stream_id("https://x/y.m3u8"), None);
    }

    /// Live end-to-end check against Vavoo's API (network, ids rotate) — run
    /// explicitly with `cargo test -p iris-api vavoo_live -- --ignored`.
    #[tokio::test]
    #[ignore = "hits the live Vavoo API"]
    async fn vavoo_live_catalog_and_resolve() {
        let http = reqwest::Client::builder().build().unwrap();
        let v = Vavoo::default();
        let entries = v.entries_for_groups(&http, &["France".to_string()]).await;
        assert!(!entries.is_empty(), "France catalog should not be empty");
        assert!(
            entries.iter().any(|e| e.name.eq_ignore_ascii_case("M6")),
            "M6 expected in France catalog"
        );
        let id = stream_id(&entries[0].url).expect("vavoo:// url");
        let resolved = v.resolve(&http, id).await.expect("resolve should succeed");
        assert!(resolved.starts_with("http"), "resolved to {resolved}");
    }

    #[test]
    fn clean_name_strips_provider_tag_only() {
        assert_eq!(clean_name("M6 .c"), "M6");
        assert_eq!(clean_name("M6 HD .s"), "M6 HD");
        assert_eq!(clean_name("M6 FHD .b"), "M6 FHD");
        assert_eq!(clean_name("M6 MUSIC (BACKUP) .s"), "M6 MUSIC (BACKUP)");
        // no provider tag → unchanged
        assert_eq!(clean_name("France 2"), "France 2");
        // a genuine trailing token that isn't a ' .x' marker stays put
        assert_eq!(clean_name("Canal 32"), "Canal 32");
    }
}
