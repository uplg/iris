use std::fmt::Write as _;

use iris_config::ProviderEntry;
use iris_core::Error;
use iris_core::search::{MediaKind, SearchQuery};

/// Browser user agent sent by the scraping/JSON providers (some trackers
/// sit behind a WAF that refuses non-browser agents).
pub(crate) const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:150.0) Gecko/20100101 Firefox/150.0";

/// First byte of a valid `.torrent` file (bencoded dictionary).
pub(crate) const BENCODE_DICT_MARKER: u8 = b'd';

/// Extract a string field from a provider entry, or fall back to the env var
/// named by `<key>_env` if present. Useful for secrets that should not live
/// in `providers.toml`.
pub(crate) fn field_or_env(entry: &ProviderEntry, key: &str) -> Result<String, Error> {
    if let Some(v) = entry.fields.get(key).and_then(|v| v.as_str()) {
        return Ok(v.to_string());
    }
    let env_key = format!("{key}_env");
    if let Some(env_name) = entry.fields.get(&env_key).and_then(|v| v.as_str()) {
        return std::env::var(env_name).map_err(|_| {
            Error::Provider(format!(
                "provider `{}`: env var `{env_name}` (referenced by `{env_key}`) not set",
                entry.id
            ))
        });
    }
    Err(Error::Provider(format!(
        "provider `{}` missing required field `{key}` (or `{key}_env`)",
        entry.id
    )))
}

/// Like [`field_or_env`] but for genuinely optional fields: `Ok(None)`
/// when neither `<key>` nor `<key>_env` is declared, an error only when
/// a declared env var is missing (a misconfiguration worth surfacing).
pub(crate) fn optional_field_or_env(
    entry: &ProviderEntry,
    key: &str,
) -> Result<Option<String>, Error> {
    let declared =
        entry.fields.contains_key(key) || entry.fields.contains_key(&format!("{key}_env"));
    if declared {
        field_or_env(entry, key).map(Some)
    } else {
        Ok(None)
    }
}

/// The entry's required `base_url`, parsed; `provider` names it in errors.
pub(crate) fn base_url(entry: &ProviderEntry, provider: &str) -> Result<url::Url, Error> {
    url::Url::parse(field_str(entry, "base_url")?)
        .map_err(|e| Error::Provider(format!("{provider} base_url invalid: {e}")))
}

pub(crate) fn field_str<'a>(entry: &'a ProviderEntry, key: &str) -> Result<&'a str, Error> {
    entry
        .fields
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            Error::Provider(format!(
                "provider `{}` missing required field `{key}`",
                entry.id
            ))
        })
}

/// The search text sent to a tracker's substring filter. When the SCENE
/// parser extracted a title + season (+ episode) from the raw query, rebuild
/// the canonical SCENE form trackers match verbatim (`Classroom of the Elite
/// S04E11`); otherwise the raw `q` — including a parsed title without S/E,
/// which keeps any year / qualifier the parsed title alone would lose.
pub(crate) fn scene_query(q: &SearchQuery) -> String {
    match (q.parsed_title.as_deref(), q.season, q.episode) {
        (Some(t), Some(s), Some(e)) if !t.is_empty() && e > 0 => format!("{t} S{s:02}E{e:02}"),
        (Some(t), Some(s), _) if !t.is_empty() => format!("{t} S{s:02}"),
        _ => q.q.clone(),
    }
}

/// A scraped tracker's movie / TV category ids.
pub(crate) struct KindCategories {
    pub movie: &'static [u32],
    pub tv: &'static [u32],
}

impl KindCategories {
    pub(crate) fn kind_of(&self, id: u32) -> Option<MediaKind> {
        if self.movie.contains(&id) {
            Some(MediaKind::Movie)
        } else if self.tv.contains(&id) {
            Some(MediaKind::Tv)
        } else {
            None
        }
    }

    /// The ids a search sends: the kind's own, both lists without one.
    pub(crate) fn for_kind(&self, kind: Option<MediaKind>) -> Vec<u32> {
        match kind {
            Some(MediaKind::Movie) => self.movie.to_vec(),
            Some(MediaKind::Tv) => self.tv.to_vec(),
            None => [self.movie, self.tv].concat(),
        }
    }
}

/// `"{parent} / {sub}"`, or whichever of the two a tracker filled (the
/// subcategory alone when it repeats the parent).
pub(crate) fn join_category(parent: Option<String>, sub: Option<String>) -> Option<String> {
    match (parent, sub) {
        (Some(p), Some(s)) if p != s => Some(format!("{p} / {s}")),
        (Some(p), _) => Some(p),
        (None, s) => s,
    }
}

/// An RSS `<pubDate>` (RFC 2822). Feeds drift (missing weekday, `GMT`
/// instead of `+0000`); `None` is benign, the UI shows "unknown date".
pub(crate) fn parse_rfc2822(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc2822(s.trim())
        .ok()
        .map(|d| d.with_timezone(&chrono::Utc))
}

/// Best-effort year extraction from a release title: take the first 4-digit
/// substring in the 1900..=2099 range.
pub(crate) fn extract_year(title: &str) -> Option<u16> {
    let bytes = title.as_bytes();
    let mut i = 0;
    while i + 4 <= bytes.len() {
        if bytes[i].is_ascii_digit() {
            // ensure no digit immediately before/after — avoid matching "20091" etc.
            let before_ok = i == 0 || !bytes[i - 1].is_ascii_digit();
            let after_ok = i + 4 == bytes.len() || !bytes[i + 4].is_ascii_digit();
            if before_ok
                && after_ok
                && bytes[i + 1].is_ascii_digit()
                && bytes[i + 2].is_ascii_digit()
                && bytes[i + 3].is_ascii_digit()
                && let Ok(s) = std::str::from_utf8(&bytes[i..i + 4])
                && let Ok(n) = s.parse::<u16>()
                && (1900..=2099).contains(&n)
            {
                return Some(n);
            }
        }
        i += 1;
    }
    None
}

/// Prowlarr's `ParseUtil.GetBytes` equivalent: `9.14 GiB` / `700 MB` →
/// bytes, binary multipliers for both `XiB` and `XB` spellings.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn parse_size(text: &str) -> Option<u64> {
    let t = text.trim();
    let split = t.find(|c: char| c.is_ascii_alphabetic())?;
    let num: f64 = t[..split].trim().replace(',', "").parse().ok()?;
    let mult: f64 = match t[split..].trim().to_ascii_lowercase().as_str() {
        "b" => 1.0,
        "kb" | "kib" => 1024.0,
        "mb" | "mib" => 1024.0 * 1024.0,
        "gb" | "gib" => 1024.0 * 1024.0 * 1024.0,
        "tb" | "tib" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    if num < 0.0 {
        return None;
    }
    Some((num * mult).round() as u64)
}

/// Run a CPU-bound page parse on the blocking pool. A tracker page is a few
/// hundred KB of HTML, and `scraper` builds the whole DOM: on an async worker
/// that would stall every request scheduled alongside it.
pub(crate) async fn parse_off_thread<T, F>(parse: F) -> Result<T, Error>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    tokio::task::spawn_blocking(parse)
        .await
        .map_err(|e| Error::Provider(format!("page parse task: {e}")))
}

/// Normalise an `info_hash` string into a canonical 40-char lowercase
/// hex SHA-1. Returns `None` on any unrecognised shape so a rogue
/// value can't poison downstream identity comparisons.
///
/// Two encodings observed in the wild:
///   * 40 hex chars — the canonical form (`/api/torrents/{id}`,
///     mainline `UNIT3D` search rows). Pass-through.
///   * 80 hex chars — `/api/torrents/filter` ships the infohash
///     hex-encoded a SECOND time: each of the 40 hex chars is
///     interpreted as a raw byte and re-hex-encoded, doubling the
///     length. Decode the outer layer, verify the inner is itself
///     a clean 40-char hex string.
///   * 32 base32 chars — the BEP 9 magnet `xt` alternative some Torznab
///     indexers echo in the `infohash` attr. Decoded to hex.
pub(crate) fn normalize_infohash(raw: &str) -> Option<String> {
    let s = raw.trim().to_ascii_lowercase();
    if iris_core::ids::is_infohash_hex(&s) {
        return Some(s);
    }
    if s.len() == 80 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
        let mut inner = String::with_capacity(40);
        for chunk in s.as_bytes().as_chunks::<2>().0 {
            let hi = (chunk[0] as char).to_digit(16)?;
            let lo = (chunk[1] as char).to_digit(16)?;
            let byte = u8::try_from((hi << 4) | lo).ok()?;
            // Each decoded byte must itself be an ASCII hex digit —
            // otherwise this isn't the double-encoded form and emitting
            // it as an "infohash" would feed librqbit garbage.
            if !byte.is_ascii_hexdigit() {
                return None;
            }
            inner.push(byte as char);
        }
        return Some(inner);
    }
    if s.len() == 32 {
        return base32_infohash(&s);
    }
    None
}

fn base32_infohash(s: &str) -> Option<String> {
    let mut bits: u64 = 0;
    let mut nbits = 0u32;
    let mut out = String::with_capacity(40);
    for c in s.bytes() {
        let v = match c {
            b'a'..=b'z' => c - b'a',
            b'2'..=b'7' => c - b'2' + 26,
            _ => return None,
        };
        bits = (bits << 5) | u64::from(v);
        nbits += 5;
        if nbits >= 8 {
            nbits -= 8;
            let byte = (bits >> nbits) & 0xff;
            let _ = write!(out, "{byte:02x}");
        }
    }
    (out.len() == 40).then_some(out)
}

#[cfg(test)]
mod tests {
    use iris_core::search::SearchQuery;

    use super::{extract_year, join_category, normalize_infohash, scene_query};

    #[test]
    fn infohash_accepts_hex_and_base32_and_rejects_the_rest() {
        let hex = "c12fe1c06bba254a9dc9f519b335aa7c1367a88a";
        assert_eq!(
            normalize_infohash(&hex.to_ascii_uppercase()).as_deref(),
            Some(hex)
        );
        assert_eq!(
            normalize_infohash("YEX6DQDLXISUVHOJ6UM3GNNKPQJWPKEK").as_deref(),
            Some(hex)
        );
        assert_eq!(normalize_infohash(&"a".repeat(64)), None);
        assert_eq!(normalize_infohash("YEX6DQDLXISUVHOJ6UM3GNNKPQJWPKE1"), None);
        assert_eq!(normalize_infohash("not a hash"), None);
    }

    fn parsed(title: Option<&str>, season: Option<u32>, episode: Option<u32>) -> SearchQuery {
        SearchQuery {
            q: "raw query 2017".into(),
            parsed_title: title.map(str::to_owned),
            season,
            episode,
            ..SearchQuery::default()
        }
    }

    #[test]
    fn scene_query_rebuilds_the_scene_form_only_with_a_season() {
        let t = Some("Classroom of the Elite");
        assert_eq!(
            scene_query(&parsed(t, Some(4), Some(11))),
            "Classroom of the Elite S04E11"
        );
        assert_eq!(
            scene_query(&parsed(t, Some(4), None)),
            "Classroom of the Elite S04"
        );
        assert_eq!(
            scene_query(&parsed(t, Some(4), Some(0))),
            "Classroom of the Elite S04"
        );
        assert_eq!(scene_query(&parsed(t, None, None)), "raw query 2017");
        assert_eq!(
            scene_query(&parsed(Some(""), Some(1), Some(2))),
            "raw query 2017"
        );
        assert_eq!(
            scene_query(&parsed(None, Some(1), Some(2))),
            "raw query 2017"
        );
    }

    #[test]
    fn category_joins_parent_and_distinct_sub() {
        let s = |v: &str| Some(v.to_owned());
        assert_eq!(
            join_category(s("Films"), s("WEB")).as_deref(),
            Some("Films / WEB")
        );
        assert_eq!(
            join_category(s("Films"), s("Films")).as_deref(),
            Some("Films")
        );
        assert_eq!(join_category(s("Films"), None).as_deref(), Some("Films"));
        assert_eq!(join_category(None, s("WEB")).as_deref(), Some("WEB"));
        assert_eq!(join_category(None, None), None);
    }

    #[test]
    fn finds_year() {
        assert_eq!(
            extract_year("Avatar 2009 EXTENDED MULTi BluRay"),
            Some(2009)
        );
        assert_eq!(extract_year("Some Movie (2024) 1080p"), Some(2024));
        assert_eq!(extract_year("No year here"), None);
        // Don't pick years embedded in larger numbers.
        assert_eq!(extract_year("Bitrate 20091 kbps"), None);
        // First valid year wins.
        assert_eq!(extract_year("Show 2018 S03 2026"), Some(2018));
    }
}
