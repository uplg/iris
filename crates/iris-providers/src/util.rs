use iris_config::ProviderEntry;
use iris_core::Error;

/// Extract a string field from a provider entry, or fall back to the env var
/// named by `<key>_env` if present. Useful for secrets that should not live
/// in `providers.toml`.
pub(crate) fn field_or_env(
    entry: &ProviderEntry,
    key: &str,
) -> Result<String, Error> {
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
            {
                if let Ok(s) = std::str::from_utf8(&bytes[i..i + 4]) {
                    if let Ok(n) = s.parse::<u16>() {
                        if (1900..=2099).contains(&n) {
                            return Some(n);
                        }
                    }
                }
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::extract_year;

    #[test]
    fn finds_year() {
        assert_eq!(extract_year("Avatar 2009 EXTENDED MULTi BluRay"), Some(2009));
        assert_eq!(extract_year("Some Movie (2024) 1080p"), Some(2024));
        assert_eq!(extract_year("No year here"), None);
        // Don't pick years embedded in larger numbers.
        assert_eq!(extract_year("Bitrate 20091 kbps"), None);
        // First valid year wins.
        assert_eq!(extract_year("Show 2018 S03 2026"), Some(2018));
    }
}
