//! HLS playlist rewriting + proxy-URL signing for Live TV.
//!
//! Every URI in an upstream playlist (variant playlists, segments, keys,
//! init maps, alternate renditions) is rewritten to the authenticated
//! `/api/livetv/proxy` endpoint with an HMAC over the target URL. The proxy
//! only fetches URLs carrying a valid signature — i.e. URLs the server
//! itself minted while rewriting a playlist it fetched — so an authenticated
//! user cannot turn the endpoint into an open proxy / SSRF primitive.
//! (DNS-rebinding-grade SSRF from a playlist-controlled host is accepted
//! residual risk for an auth-gated household server.)

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use url::Url;

type HmacSha256 = Hmac<Sha256>;

/// Domain separation so a leaked signature can't be replayed against any
/// other HMAC use of the JWT secret (and vice versa).
const KEY_PREFIX: &[u8] = b"iris-livetv-proxy-v1";

/// Longest accepted base64url-encoded upstream URL (defense in depth).
pub const MAX_ENCODED_URL: usize = 4096;

#[derive(Clone)]
pub struct Signer {
    key: Vec<u8>,
}

impl Signer {
    pub fn new(jwt_secret: &str) -> Self {
        let mut key = Vec::with_capacity(KEY_PREFIX.len() + jwt_secret.len());
        key.extend_from_slice(KEY_PREFIX);
        key.extend_from_slice(jwt_secret.as_bytes());
        Self { key }
    }

    fn mac(&self, channel_key: &str, url: &str) -> HmacSha256 {
        let mut mac = HmacSha256::new_from_slice(&self.key).expect("hmac accepts any key size");
        mac.update(channel_key.as_bytes());
        mac.update(b"|");
        mac.update(url.as_bytes());
        mac
    }

    pub fn sign(&self, channel_key: &str, url: &str) -> String {
        hex::encode(self.mac(channel_key, url).finalize().into_bytes())
    }

    /// Constant-time verification.
    pub fn verify(&self, channel_key: &str, url: &str, sig_hex: &str) -> bool {
        let Ok(sig) = hex::decode(sig_hex) else {
            return false;
        };
        self.mac(channel_key, url).verify_slice(&sig).is_ok()
    }
}

/// Decode the `u` query param back to the upstream URL. Enforces the size
/// cap and an http(s) scheme.
pub fn decode_upstream(encoded: &str) -> Option<Url> {
    if encoded.len() > MAX_ENCODED_URL {
        return None;
    }
    let bytes = URL_SAFE_NO_PAD.decode(encoded).ok()?;
    let raw = String::from_utf8(bytes).ok()?;
    let url = Url::parse(&raw).ok()?;
    matches!(url.scheme(), "http" | "https").then_some(url)
}

/// `channel_key` is `country:channel_id` — it namespaces signatures per
/// channel so the proxy can apply that channel's upstream headers.
pub fn proxied_url(channel_key: &str, upstream: &Url, signer: &Signer) -> String {
    let u = URL_SAFE_NO_PAD.encode(upstream.as_str());
    let s = signer.sign(channel_key, upstream.as_str());
    format!("/api/livetv/proxy?c={channel_key}&u={u}&s={s}")
}

/// Signing namespace for proxied channel logos (distinct from any channel
/// key — those always contain `:`).
pub const LOGO_KEY: &str = "logo";

/// Signed same-origin URL for a channel logo. Serving logos through the
/// backend kills the hotlink CORS noise and lets clients read pixels for
/// the luminance-adaptive plate.
pub fn logo_url(upstream: &Url, signer: &Signer) -> String {
    let u = URL_SAFE_NO_PAD.encode(upstream.as_str());
    let s = signer.sign(LOGO_KEY, upstream.as_str());
    format!("/api/livetv/logo?u={u}&s={s}")
}

/// Rewrite every URI in an HLS playlist (master or media — the tag set
/// decides, not us) to the signed proxy endpoint. `base` must be the
/// *final* URL the playlist was fetched from (post-redirect), or relative
/// segment URIs resolve against the wrong host.
pub fn rewrite_playlist(body: &str, base: &Url, channel_key: &str, signer: &Signer) -> String {
    let mut out = String::with_capacity(body.len() * 2);
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            out.push('\n');
            continue;
        }
        if trimmed.starts_with('#') {
            // IPTV restream masters routinely LIE in `CODECS=` — e.g. a TS
            // carrying H.264 + E-AC-3 declared as `CODECS="avc1.640028"`
            // (video only). Players trust the attribute over the content:
            // hls.js then builds no audio pipeline and Mediabunny's HLS
            // input doesn't even expose the track. Strip it so engines
            // sniff the actual TS instead.
            let line = if trimmed.starts_with("#EXT-X-STREAM-INF") {
                strip_codecs_attr(trimmed)
            } else {
                trimmed.to_string()
            };
            // Tags carrying a URI="…" attribute (EXT-X-KEY, EXT-X-MAP,
            // EXT-X-MEDIA, EXT-X-I-FRAME-STREAM-INF, …) need that URI
            // proxied too — an unrewritten AES key URI breaks playback
            // silently (cross-origin key fetch fails).
            out.push_str(&rewrite_uri_attr(&line, base, channel_key, signer));
        } else {
            match base.join(trimmed) {
                Ok(abs) if matches!(abs.scheme(), "http" | "https") => {
                    out.push_str(&proxied_url(channel_key, &abs, signer));
                }
                // data: URIs and unparseable lines pass through untouched.
                _ => out.push_str(trimmed),
            }
        }
        out.push('\n');
    }
    out
}

/// Drop the `CODECS="…"` attribute from an `#EXT-X-STREAM-INF` line (with
/// whichever comma glued it to its neighbours).
fn strip_codecs_attr(line: &str) -> String {
    let Some(start) = line.find("CODECS=\"") else {
        return line.to_string();
    };
    let val_start = start + 8;
    let Some(val_len) = line[val_start..].find('"') else {
        return line.to_string();
    };
    let mut end = val_start + val_len + 1;
    let mut begin = start;
    // Consume one adjoining comma (trailing preferred, else leading).
    if line[end..].starts_with(',') {
        end += 1;
    } else if begin > 0 && line[..begin].ends_with(',') {
        begin -= 1;
    }
    format!("{}{}", &line[..begin], &line[end..])
}

/// Rewrite the value of a `URI="…"` attribute inside a tag line, if any.
fn rewrite_uri_attr(line: &str, base: &Url, channel_key: &str, signer: &Signer) -> String {
    let Some(start) = line.find("URI=\"") else {
        return line.to_string();
    };
    let val_start = start + 5;
    let Some(val_len) = line[val_start..].find('"') else {
        return line.to_string();
    };
    let uri = &line[val_start..val_start + val_len];
    let Ok(abs) = base.join(uri) else {
        return line.to_string();
    };
    if !matches!(abs.scheme(), "http" | "https") {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len() + 128);
    out.push_str(&line[..val_start]);
    out.push_str(&proxied_url(channel_key, &abs, signer));
    out.push_str(&line[val_start + val_len..]);
    out
}

/// Whether a proxied response body is itself a playlist that must be
/// re-rewritten (media playlists reached through the master).
pub fn is_playlist(url: &Url, content_type: Option<&str>) -> bool {
    if content_type.is_some_and(|ct| {
        let ct = ct.to_ascii_lowercase();
        ct.contains("mpegurl") || ct.contains("application/x-mpegurl")
    }) {
        return true;
    }
    std::path::Path::new(url.path())
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("m3u8") || e.eq_ignore_ascii_case("m3u"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signer() -> Signer {
        Signer::new("test-secret")
    }

    #[test]
    fn sign_verify_roundtrip_and_tamper_rejection() {
        let s = signer();
        let sig = s.sign("fr:tf1", "http://up.example/seg1.ts");
        assert!(s.verify("fr:tf1", "http://up.example/seg1.ts", &sig));
        // tampered URL, channel, signature
        assert!(!s.verify("fr:tf1", "http://up.example/seg2.ts", &sig));
        assert!(!s.verify("fr:m6", "http://up.example/seg1.ts", &sig));
        assert!(!s.verify("fr:tf1", "http://up.example/seg1.ts", "deadbeef"));
        assert!(!s.verify("fr:tf1", "http://up.example/seg1.ts", "not-hex"));
        // different secret
        assert!(!Signer::new("other").verify("fr:tf1", "http://up.example/seg1.ts", &sig));
    }

    #[test]
    fn proxied_url_roundtrips_through_decode() {
        let s = signer();
        let up = Url::parse("http://up.example/live/playlist.m3u8?token=abc&x=1").unwrap();
        let proxied = proxied_url("fr:tf1", &up, &s);
        let u_param = proxied.split("u=").nth(1).unwrap().split('&').next().unwrap();
        let decoded = decode_upstream(u_param).unwrap();
        assert_eq!(decoded, up);
    }

    #[test]
    fn decode_upstream_rejects_bad_input() {
        assert!(decode_upstream(&"A".repeat(MAX_ENCODED_URL + 1)).is_none());
        assert!(decode_upstream("!!!not-base64!!!").is_none());
        let ftp = URL_SAFE_NO_PAD.encode("ftp://up.example/x");
        assert!(decode_upstream(&ftp).is_none());
        let file = URL_SAFE_NO_PAD.encode("file:///etc/passwd");
        assert!(decode_upstream(&file).is_none());
    }

    #[test]
    fn rewrites_master_playlist_uris() {
        let s = signer();
        let base = Url::parse("https://cdn.example/live/tf1/master.m3u8").unwrap();
        let body = "#EXTM3U\n\
            #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"French\",URI=\"audio/fr.m3u8\"\n\
            #EXT-X-STREAM-INF:BANDWIDTH=5000000,AUDIO=\"aud\"\n\
            hd/index.m3u8\n\
            #EXT-X-STREAM-INF:BANDWIDTH=1000000\n\
            https://other-cdn.example/sd/index.m3u8\n";
        let out = rewrite_playlist(body, &base, "fr:tf1", &s);
        // relative variant resolved against base dir
        let variant_rel = Url::parse("https://cdn.example/live/tf1/hd/index.m3u8").unwrap();
        assert!(out.contains(&proxied_url("fr:tf1", &variant_rel, &s)));
        // absolute variant on another host still proxied
        let variant_abs = Url::parse("https://other-cdn.example/sd/index.m3u8").unwrap();
        assert!(out.contains(&proxied_url("fr:tf1", &variant_abs, &s)));
        // EXT-X-MEDIA URI attribute rewritten
        let expected_audio = Url::parse("https://cdn.example/live/tf1/audio/fr.m3u8").unwrap();
        assert!(out.contains(&format!("URI=\"{}\"", proxied_url("fr:tf1", &expected_audio, &s))));
        // stream-inf line itself untouched
        assert!(out.contains("#EXT-X-STREAM-INF:BANDWIDTH=5000000,AUDIO=\"aud\"\n"));
        // no upstream URL left in clear
        assert!(!out.contains("hd/index.m3u8\n"));
    }

    #[test]
    fn rewrites_media_playlist_segments_keys_and_maps() {
        let s = signer();
        let base = Url::parse("http://cdn.example/hls/ch/index.m3u8?tok=1").unwrap();
        let body = "#EXTM3U\n\
            #EXT-X-TARGETDURATION:6\n\
            #EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\",IV=0xabc\n\
            #EXT-X-MAP:URI=\"init.mp4\"\n\
            #EXTINF:6.0,\n\
            seg001.ts?tok=1\n\
            #EXT-X-DISCONTINUITY\n\
            #EXTINF:6.0,\n\
            /abs/seg002.ts\n";
        let out = rewrite_playlist(body, &base, "fr:m6", &s);
        let key = Url::parse("http://cdn.example/hls/ch/key.bin").unwrap();
        let map = Url::parse("http://cdn.example/hls/ch/init.mp4").unwrap();
        let seg_rel = Url::parse("http://cdn.example/hls/ch/seg001.ts?tok=1").unwrap();
        let seg_abs = Url::parse("http://cdn.example/abs/seg002.ts").unwrap();
        for u in [&key, &map, &seg_rel, &seg_abs] {
            assert!(out.contains(&proxied_url("fr:m6", u, &s)), "missing rewrite for {u}");
        }
        // non-URI tags untouched
        assert!(out.contains("#EXT-X-TARGETDURATION:6\n"));
        assert!(out.contains("#EXT-X-DISCONTINUITY\n"));
        // KEY attrs around the URI survive
        assert!(out.contains("METHOD=AES-128,URI=\""));
        assert!(out.contains("\",IV=0xabc"));
    }

    #[test]
    fn strips_lying_codecs_attribute() {
        let s = signer();
        let base = Url::parse("https://cdn.example/live/m6/master.m3u8").unwrap();
        // The M6 shape: TS carries H.264 + E-AC-3 but CODECS declares video
        // only. The attribute must go so players sniff the real content.
        let body = "#EXTM3U\n\
            #EXT-X-STREAM-INF:AVERAGE-BANDWIDTH=4470000,BANDWIDTH=5580000,RESOLUTION=1920x1080,FRAME-RATE=25.000,CODECS=\"avc1.640028\",CLOSED-CAPTIONS=NONE\n\
            tracks-v1a1/mono.ts.m3u8\n";
        let out = rewrite_playlist(body, &base, "fr:m6", &s);
        assert!(!out.contains("CODECS"));
        assert!(out.contains("FRAME-RATE=25.000,CLOSED-CAPTIONS=NONE"));
        // trailing-position CODECS loses its leading comma instead
        assert_eq!(
            strip_codecs_attr("#EXT-X-STREAM-INF:BANDWIDTH=1,CODECS=\"avc1,mp4a.40.2\""),
            "#EXT-X-STREAM-INF:BANDWIDTH=1"
        );
        assert_eq!(strip_codecs_attr("#EXT-X-STREAM-INF:BANDWIDTH=1"), "#EXT-X-STREAM-INF:BANDWIDTH=1");
    }

    #[test]
    fn data_uris_pass_through() {
        let s = signer();
        let base = Url::parse("https://cdn.example/x.m3u8").unwrap();
        let body = "#EXT-X-KEY:METHOD=AES-128,URI=\"data:text/plain;base64,AAAA\"\n";
        let out = rewrite_playlist(body, &base, "fr:x", &s);
        assert!(out.contains("URI=\"data:text/plain;base64,AAAA\""));
    }

    #[test]
    fn is_playlist_by_content_type_or_extension() {
        let u = Url::parse("http://x/chunk.ts").unwrap();
        assert!(is_playlist(&u, Some("application/vnd.apple.mpegurl")));
        assert!(!is_playlist(&u, Some("video/mp2t")));
        assert!(!is_playlist(&u, None));
        let m = Url::parse("http://x/media.m3u8?sig=1").unwrap();
        assert!(is_playlist(&m, None));
    }
}
