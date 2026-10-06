use iris_config::ProviderEntry;
use iris_core::Error;
use iris_core::search::SearchQuery;

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

#[cfg(test)]
mod tests {
    use iris_core::search::SearchQuery;

    use super::{extract_year, scene_query};

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
