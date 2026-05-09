// Bitrate / duration parsing pulls bounded numbers out of MediaInfo
// dumps (always positive, always under u32::MAX kbps). Noise lints from
// our key-value extractor pattern (`.map(|s| s.to_string())` everywhere)
// also silenced — they're explicit on purpose for readability.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::redundant_closure_for_method_calls,
    clippy::implicit_clone,
    clippy::ptr_arg,
    // Doc-only style: reading this file's prose with backticks around
    // every word like `MediaInfo`, `SDH`, `Audio` makes it harder to
    // skim, not easier. Keep narrative prose plain.
    clippy::doc_markdown,
)]

//! MediaInfo NFO parser.
//!
//! Indexers ship a MediaInfo dump with each torrent — a long key:value
//! text block split into "General", "Video", "Audio #N", "Text #N" and
//! "Menu" sections. We parse it server-side into a `MediaInfoSummary` so
//! the web + TV preview dialogs can render structured "facts grids"
//! without having to ship a regex tower in two languages.
//!
//! The parser is intentionally lenient: missing fields stay `None`, the
//! text format isn't machine-readable in a strict sense and providers
//! tweak the layout (encoder version differences). When in doubt we drop
//! the value rather than guess.
//!
//! Extending the parser:
//!   * add a field to the matching struct in `iris-core/search.rs`
//!   * add a regex / line-extractor below
//!   * keep behaviour additive — older NFOs missing the new field stay
//!     valid

use iris_core::search::{AudioInfo, MediaInfoSummary, SubInfo, VideoInfo};

/// Parse a MediaInfo dump into a structured summary. Returns `None` if
/// the input doesn't look like MediaInfo at all (no recognisable section
/// headers) so callers can pass arbitrary text without false positives.
pub fn parse(nfo: &str) -> Option<MediaInfoSummary> {
    let blocks = split_into_blocks(nfo);
    if blocks.is_empty() {
        return None;
    }

    let mut out = MediaInfoSummary::default();
    for (header, body) in blocks {
        match section_kind(&header) {
            Section::Video if out.video.is_none() => {
                out.video = Some(parse_video(&body));
            }
            Section::Audio => {
                out.audio.push(parse_audio(&body));
            }
            Section::Text => {
                out.subtitles.push(parse_sub(&body));
            }
            _ => {}
        }
    }
    Some(out)
}

/// Split the NFO into `(header, body)` chunks. A new chunk starts at any
/// line that's a section header on its own (the lines between such
/// headers and the next blank line are the body).
fn split_into_blocks(nfo: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut current_header: Option<String> = None;
    let mut current_body = String::new();
    for raw_line in nfo.lines() {
        let line = raw_line.trim_end_matches('\r');
        if is_section_header(line) {
            if let Some(h) = current_header.take() {
                out.push((h, std::mem::take(&mut current_body)));
            }
            current_header = Some(line.trim().to_string());
        } else if current_header.is_some() {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }
    if let Some(h) = current_header {
        out.push((h, current_body));
    }
    out
}

/// MediaInfo section headers are bare lines like `General`, `Video`,
/// `Audio`, `Audio #2`, `Text #5`, `Menu`. Discriminate from key:value
/// lines (which always contain a colon).
fn is_section_header(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.contains(':') || trimmed.starts_with(' ') {
        return false;
    }
    matches!(
        trimmed.split('#').next().map(str::trim),
        Some("General" | "Video" | "Audio" | "Text" | "Menu" | "Image" | "Other"),
    )
}

enum Section {
    Video,
    Audio,
    Text,
    Other,
}

fn section_kind(header: &str) -> Section {
    let head = header.split('#').next().unwrap_or(header).trim();
    match head {
        "Video" => Section::Video,
        "Audio" => Section::Audio,
        "Text" => Section::Text,
        _ => Section::Other,
    }
}

fn parse_video(body: &str) -> VideoInfo {
    let kv = key_value_map(body);
    let resolution = match (kv.get("Width").and_then(parse_pixels), kv.get("Height").and_then(parse_pixels)) {
        (Some(w), Some(h)) => Some(format!("{w}x{h}")),
        _ => None,
    };
    let duration_secs = kv.get("Duration").and_then(|s| parse_duration_secs(s));
    let fps = kv
        .get("Frame rate")
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.replace(',', ".").parse::<f32>().ok());
    let bitrate_kbps = kv.get("Bit rate").and_then(|s| parse_bitrate_kbps(s));
    let codec = kv
        .get("Format")
        .or_else(|| kv.get("Commercial name"))
        .map(|s| s.to_string());
    // Best-effort HDR detection: the "HDR format" line shows up on HDR10
    // and DV releases; otherwise check transfer characteristics.
    let hdr = kv
        .get("HDR format")
        .map(|s| s.to_string())
        .or_else(|| {
            let xfer = kv.get("Transfer characteristics").map_or("", String::as_str);
            if xfer.contains("PQ") || xfer.contains("2084") {
                Some("HDR10".to_string())
            } else if xfer.contains("HLG") {
                Some("HLG".to_string())
            } else {
                None
            }
        });
    VideoInfo {
        codec,
        resolution,
        duration_secs,
        fps,
        bitrate_kbps,
        hdr,
    }
}

fn parse_audio(body: &str) -> AudioInfo {
    let kv = key_value_map(body);
    let lang = kv.get("Language").map(|s| s.to_string());
    let codec = kv
        .get("Format")
        .or_else(|| kv.get("Commercial name"))
        .map(|s| s.to_string());
    let channels = kv.get("Channel(s)").and_then(|s| {
        s.split_whitespace().next().and_then(|s| s.parse::<u8>().ok())
    });
    let bitrate_kbps = kv.get("Bit rate").and_then(|s| parse_bitrate_kbps(s));
    let title = kv.get("Title").map(|s| s.to_string());
    let default = kv.get("Default").is_some_and(|s| s.eq_ignore_ascii_case("yes"));
    let commercial_name = kv.get("Commercial name").map(|s| s.to_string());
    AudioInfo {
        lang,
        codec,
        channels,
        bitrate_kbps,
        title,
        default,
        commercial_name,
    }
}

fn parse_sub(body: &str) -> SubInfo {
    let kv = key_value_map(body);
    let lang = kv.get("Language").map(|s| s.to_string());
    let format = kv.get("Format").map(|s| s.to_string());
    let title = kv.get("Title").map(|s| s.to_string());
    let default = kv.get("Default").is_some_and(|s| s.eq_ignore_ascii_case("yes"));
    let forced = kv.get("Forced").is_some_and(|s| s.eq_ignore_ascii_case("yes"));
    SubInfo {
        lang,
        format,
        title,
        default,
        forced,
    }
}

/// Build a `key → value` map from a MediaInfo block body. Lines look
/// like `Bit rate                                 : 2 100 kb/s` — split
/// on the first `:`, trim both sides.
fn key_value_map(body: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for line in body.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let key = k.trim().to_string();
        let val = v.trim().to_string();
        if key.is_empty() || val.is_empty() {
            continue;
        }
        // First occurrence wins — some keys (e.g., "Dialog Normalization")
        // appear multiple times with average/min/max suffixes.
        out.entry(key).or_insert(val);
    }
    out
}

/// `"1 920 pixels"` / `"1080 pixels"` → 1920 / 1080.
fn parse_pixels(value: &String) -> Option<u32> {
    value
        .replace([' ', '\u{00a0}'], "")
        .trim_end_matches("pixels")
        .trim_end_matches("px")
        .trim()
        .parse::<u32>()
        .ok()
}

/// `"640 kb/s"` / `"2 100 kb/s"` / `"4 151 kb/s"` → kbps.
fn parse_bitrate_kbps(value: &str) -> Option<u32> {
    // Tolerate the no-break space MediaInfo sometimes uses as a
    // thousands separator on French locale dumps.
    let cleaned: String = value
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    cleaned.parse::<u32>().ok().or_else(|| {
        cleaned
            .replace(',', ".")
            .parse::<f64>()
            .ok()
            .map(|f| f.round() as u32)
    })
}

/// `"2 h 8 min"` / `"1 h 58 min"` / `"58 min"` → seconds.
/// The format isn't strict — older MediaInfo dumps use `1h 58mn 12s`,
/// newer ones use the spaced form. Parse what we can, drop the rest.
fn parse_duration_secs(value: &str) -> Option<u32> {
    let mut total = 0u32;
    let mut last_num: Option<u32> = None;
    for tok in value.split_whitespace() {
        if let Ok(n) = tok.parse::<u32>() {
            last_num = Some(n);
            continue;
        }
        let Some(n) = last_num.take() else { continue };
        let unit = tok.to_ascii_lowercase();
        // Order matters — match longer units first so "min" doesn't
        // greedily eat the "m" of "ms".
        if unit.starts_with("ms") {
            // ignore millisecond fractions
        } else if unit.starts_with('h') {
            total += n * 3600;
        } else if unit.starts_with("min") || unit.starts_with("mn") || unit == "m" {
            total += n * 60;
        } else if unit.starts_with('s') {
            total += n;
        }
    }
    if total == 0 { None } else { Some(total) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_NFO: &str = "General\r\nComplete name                            : My.Dearest.Assassin\r\nFormat                                   : Matroska\r\nDuration                                 : 2 h 8 min\r\nOverall bit rate                         : 4 151 kb/s\r\n\r\nVideo\r\nFormat                                   : HEVC\r\nDuration                                 : 2 h 8 min\r\nBit rate                                 : 2 100 kb/s\r\nWidth                                    : 1 920 pixels\r\nHeight                                   : 1 080 pixels\r\nFrame rate                               : 24.000 FPS\r\nDefault                                  : Yes\r\n\r\nAudio #1\r\nFormat                                   : E-AC-3\r\nCommercial name                          : Dolby Digital Plus\r\nDuration                                 : 2 h 8 min\r\nBit rate                                 : 640 kb/s\r\nChannel(s)                               : 6 channels\r\nTitle                                    : VFF\r\nLanguage                                 : French (FR)\r\nDefault                                  : Yes\r\n\r\nAudio #2\r\nFormat                                   : E-AC-3 JOC\r\nCommercial name                          : Dolby Digital Plus with Dolby Atmos\r\nBit rate                                 : 768 kb/s\r\nChannel(s)                               : 6 channels\r\nLanguage                                 : Thai\r\nDefault                                  : No\r\n\r\nText #1\r\nFormat                                   : UTF-8\r\nLanguage                                 : French (FR)\r\nTitle                                    : VFF (Forced)\r\nDefault                                  : Yes\r\nForced                                   : Yes\r\n\r\nText #2\r\nFormat                                   : UTF-8\r\nLanguage                                 : English (US)\r\nTitle                                    : (SDH)\r\nDefault                                  : No\r\nForced                                   : No\r\n";

    #[test]
    fn parses_real_world_torr9_nfo() {
        let mi = parse(SAMPLE_NFO).expect("MediaInfo summary");
        let v = mi.video.as_ref().expect("video block");
        assert_eq!(v.codec.as_deref(), Some("HEVC"));
        assert_eq!(v.resolution.as_deref(), Some("1920x1080"));
        assert_eq!(v.duration_secs, Some(2 * 3600 + 8 * 60));
        assert_eq!(v.fps, Some(24.0));
        assert_eq!(v.bitrate_kbps, Some(2100));
        assert_eq!(mi.audio.len(), 2);
        assert_eq!(mi.audio[0].lang.as_deref(), Some("French (FR)"));
        assert_eq!(mi.audio[0].channels, Some(6));
        assert_eq!(mi.audio[0].bitrate_kbps, Some(640));
        assert!(mi.audio[0].default);
        assert_eq!(
            mi.audio[1].commercial_name.as_deref(),
            Some("Dolby Digital Plus with Dolby Atmos"),
        );
        assert_eq!(mi.subtitles.len(), 2);
        assert!(mi.subtitles[0].forced);
        assert_eq!(mi.subtitles[1].lang.as_deref(), Some("English (US)"));
    }

    #[test]
    fn returns_none_on_arbitrary_text() {
        assert!(parse("Just some random description text without sections.").is_none());
    }
}
