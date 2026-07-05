//! Minimal parser for IPTV m3u playlists (iptv-org / XTVZ style).
//!
//! Only the subset those playlists actually use is handled: `#EXTINF:` lines
//! with `key="value"` attributes and a display name after the first unquoted
//! comma, optional `#EXTVLCOPT:` companion lines (per-stream http headers),
//! and the stream URL on the following non-comment line. No published crate
//! covers the attribute map + `#EXTVLCOPT` combo, hence the hand-rolled pass.

use std::collections::HashMap;

/// One playlist entry, faithful to the source (no Iris-level interpretation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct M3uEntry {
    /// Display name (text after the attribute list's unquoted comma).
    pub name: String,
    /// `key="value"` attributes from the `#EXTINF` line (`tvg-id`,
    /// `tvg-logo`, `group-title`, `http-user-agent`, …).
    pub attrs: HashMap<String, String>,
    /// `#EXTVLCOPT:key=value` lines attached to this entry
    /// (`http-user-agent`, `http-referrer`).
    pub vlc_opts: HashMap<String, String>,
    /// Stream URL.
    pub url: String,
}

impl M3uEntry {
    /// Attribute lookup, `#EXTINF` attributes winning over `#EXTVLCOPT`
    /// duplicates (they carry the same value in practice).
    pub fn header(&self, key: &str) -> Option<&str> {
        self.attrs
            .get(key)
            .or_else(|| self.vlc_opts.get(key))
            .map(String::as_str)
    }
}

/// Parse a playlist body. Unparseable entries are skipped, not fatal — a
/// single malformed upstream line must not take the whole channel list down.
pub fn parse(body: &str) -> Vec<M3uEntry> {
    let mut entries = Vec::new();
    let mut pending: Option<(String, HashMap<String, String>)> = None;
    let mut vlc_opts: HashMap<String, String> = HashMap::new();

    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            pending = parse_extinf(rest);
            vlc_opts.clear();
        } else if let Some(rest) = line.strip_prefix("#EXTVLCOPT:") {
            if let Some((key, value)) = rest.split_once('=') {
                vlc_opts.insert(key.trim().to_string(), value.trim().to_string());
            }
        } else if line.starts_with('#') {
            // #EXTM3U header, #EXTGRP, comments… — ignored.
        } else if let Some((name, attrs)) = pending.take() {
            entries.push(M3uEntry {
                name,
                attrs,
                vlc_opts: std::mem::take(&mut vlc_opts),
                url: line.to_string(),
            });
        }
    }
    entries
}

/// Parse the tail of an `#EXTINF:` line: `-1 key="value" …,Display Name`.
/// Returns `None` when no unquoted comma separates a display name.
fn parse_extinf(rest: &str) -> Option<(String, HashMap<String, String>)> {
    let mut in_quotes = false;
    let mut split = None;
    for (idx, ch) in rest.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                split = Some(idx);
                break;
            }
            _ => {}
        }
    }
    let split = split?;
    let name = rest[split + 1..].trim().to_string();
    if name.is_empty() {
        return None;
    }
    Some((name, parse_attrs(&rest[..split])))
}

/// Scan `key="value"` pairs. Keys may contain `-` (e.g. `tvg-id`,
/// `http-user-agent`); values are always double-quoted in the wild.
fn parse_attrs(section: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    let bytes = section.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Find the next `=` then require an opening quote right after it.
        let Some(eq) = section[i..].find('=').map(|p| i + p) else {
            break;
        };
        if bytes.get(eq + 1) != Some(&b'"') {
            i = eq + 1;
            continue;
        }
        // Key = identifier run ending at `=`.
        let key_start = section[..eq]
            .rfind(|c: char| c.is_whitespace())
            .map_or(0, |p| p + 1);
        let key = section[key_start..eq].trim();
        let val_start = eq + 2;
        let Some(val_end) = section[val_start..].find('"').map(|p| val_start + p) else {
            break;
        };
        if !key.is_empty() {
            attrs.insert(key.to_string(), section[val_start..val_end].to_string());
        }
        i = val_end + 1;
    }
    attrs
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"#EXTM3U
#EXTINF:-1 tvg-id="TF1.fr@SD" tvg-logo="https://i.imgur.com/QxHt9NC.png" http-user-agent="Mozilla/5.0 UA" group-title="Entertainment",TF1 (720p)
#EXTVLCOPT:http-user-agent=Mozilla/5.0 UA
https://example.com/tf1/index.m3u8
#EXTINF:-1 tvg-id="Africa24Sport.fr@SD" group-title="News;Sports",Africa 24 Sport (1080p)
https://example.com/a24s/manifest.m3u8

#EXTINF:-1 tvg-id="arte.fr@SD" group-title="Entertainment",Arte (720p) [Geo-blocked]
#EXTVLCOPT:http-referrer=https://arte.tv/
http://example.com/arte.m3u8
"#;

    #[test]
    fn parses_entries_with_attrs_and_vlcopts() {
        let entries = parse(SAMPLE);
        assert_eq!(entries.len(), 3);

        let tf1 = &entries[0];
        assert_eq!(tf1.name, "TF1 (720p)");
        assert_eq!(tf1.attrs.get("tvg-id").unwrap(), "TF1.fr@SD");
        assert_eq!(tf1.attrs.get("http-user-agent").unwrap(), "Mozilla/5.0 UA");
        assert_eq!(
            tf1.vlc_opts.get("http-user-agent").unwrap(),
            "Mozilla/5.0 UA"
        );
        assert_eq!(tf1.url, "https://example.com/tf1/index.m3u8");

        let a24 = &entries[1];
        assert_eq!(a24.attrs.get("group-title").unwrap(), "News;Sports");
        assert!(a24.vlc_opts.is_empty());

        let arte = &entries[2];
        assert_eq!(arte.name, "Arte (720p) [Geo-blocked]");
        assert_eq!(
            arte.vlc_opts.get("http-referrer").unwrap(),
            "https://arte.tv/"
        );
    }

    #[test]
    fn name_may_contain_commas_and_attrs_may_contain_commas_in_quotes() {
        let entries = parse(
            "#EXTINF:-1 tvg-id=\"X.fr\" tvg-logo=\"https://x/y,z.png\",Chaîne, la télé\nhttp://x/s.m3u8\n",
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "Chaîne, la télé");
        assert_eq!(
            entries[0].attrs.get("tvg-logo").unwrap(),
            "https://x/y,z.png"
        );
    }

    #[test]
    fn entry_without_extinf_or_without_url_is_dropped() {
        assert!(parse("https://orphan.example/s.m3u8\n").is_empty());
        assert!(parse("#EXTINF:-1 tvg-id=\"X.fr\",Name\n#EXTM3U\n").is_empty());
        // EXTINF with no display name after the comma
        assert!(parse("#EXTINF:-1 tvg-id=\"X.fr\",\nhttp://x/s.m3u8\n").is_empty());
    }

    #[test]
    fn header_prefers_extinf_attr_over_vlcopt() {
        let entries = parse(
            "#EXTINF:-1 http-user-agent=\"attr-ua\",N\n#EXTVLCOPT:http-user-agent=vlc-ua\nhttp://x/s\n",
        );
        assert_eq!(entries[0].header("http-user-agent"), Some("attr-ua"));
        assert_eq!(entries[0].header("http-referrer"), None);
    }
}
