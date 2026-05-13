//! `Iris-Caps` header — capability declaration sent by clients on every
//! playback-touching request.
//!
//! See `docs/SOTA_ARCHITECTURE.md` §2.2 for the wire format.
//!
//! Grammar (informal):
//!
//! ```text
//! header   = key-value (";" key-value)*
//! key-value = key "=" value
//! key       = [a-z][a-z0-9_-]*
//! value     = csv | boolean | ident
//! csv       = ident ("," ident)*
//! ```
//!
//! Whitespace around `;`, `,` and `=` is tolerated. Unknown keys are
//! preserved on `ClientCapabilities::extras` so future clients can advertise
//! capabilities the server doesn't recognise yet without dropping data.
//!
//! ## Example
//!
//! ```
//! use iris_caps::ClientCapabilities;
//! let caps = ClientCapabilities::parse(
//!     "container=fmp4,mkv; vdec=h264,hevc-hw,av1; webcodecs=1; webgpu=1",
//! );
//! assert!(caps.has_container("mkv"));
//! assert!(caps.has_video_decoder("hevc-hw"));
//! assert!(caps.webcodecs);
//! ```

use std::collections::BTreeMap;

use serde::Serialize;

/// The parsed `Iris-Caps` request header.
///
/// Every CSV-valued key is canonicalised to lowercase. Unknown keys land in
/// [`Self::extras`] verbatim so they are still loggable.
#[derive(Debug, Clone, Default, Serialize)]
#[allow(clippy::struct_excessive_bools)] // schema-driven flags from the wire format
pub struct ClientCapabilities {
    /// Containers the client can demux or play directly (e.g. `fmp4`, `mkv`).
    pub containers: Vec<String>,
    /// Video decoders the client can use. Values like `hevc-hw` or `av1-sw`
    /// let the client signal hardware vs software paths explicitly.
    pub video_decoders: Vec<String>,
    /// Audio decoders the client can use.
    pub audio_decoders: Vec<String>,
    /// Subtitle renderers the client supports
    /// (`webvtt`, `ass-overlay`, `pgs-overlay`, …).
    pub subtitles: Vec<String>,
    /// HDR transfer functions the client can render.
    pub hdr: Vec<String>,

    pub webcodecs: bool,
    pub webgpu: bool,
    pub mse: bool,
    /// iOS Safari Managed Media Source.
    pub mms: bool,
    /// Set when the client wants the legacy server-side HLS pipeline.
    pub legacy: bool,

    /// Free-form platform string (e.g. `web-chromium-134`, `android-tv-14`).
    pub platform: Option<String>,
    /// Bandwidth hint, free-form (`auto`, `unmetered`, `metered`, byte rate).
    pub bandwidth: Option<String>,

    /// Any key we didn't model, preserved verbatim for telemetry.
    pub extras: BTreeMap<String, String>,
}

impl ClientCapabilities {
    /// Parse an `Iris-Caps` header value. Tolerant: malformed pairs are
    /// silently dropped rather than failing the request.
    #[must_use]
    pub fn parse(input: &str) -> Self {
        let mut caps = Self::default();
        for pair in input.split(';') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();
            match key.as_str() {
                "container" => caps.containers = csv_lower(value),
                "vdec" => caps.video_decoders = csv_lower(value),
                "adec" => caps.audio_decoders = csv_lower(value),
                "subs" => caps.subtitles = csv_lower(value),
                "hdr" => caps.hdr = csv_lower(value),
                "webcodecs" => caps.webcodecs = parse_bool(value),
                "webgpu" => caps.webgpu = parse_bool(value),
                "mse" => caps.mse = parse_bool(value),
                "mms" => caps.mms = parse_bool(value),
                "legacy" => caps.legacy = parse_bool(value),
                "platform" => caps.platform = Some(value.to_owned()),
                "bandwidth" => caps.bandwidth = Some(value.to_owned()),
                _ => {
                    caps.extras.insert(key, value.to_owned());
                }
            }
        }
        caps
    }

    #[must_use]
    pub fn has_container(&self, c: &str) -> bool {
        contains_ci(&self.containers, c)
    }

    #[must_use]
    pub fn has_video_decoder(&self, c: &str) -> bool {
        contains_ci(&self.video_decoders, c)
    }

    #[must_use]
    pub fn has_audio_decoder(&self, c: &str) -> bool {
        contains_ci(&self.audio_decoders, c)
    }

    #[must_use]
    pub fn supports_subtitle(&self, c: &str) -> bool {
        contains_ci(&self.subtitles, c)
    }

    /// Serialise back to the wire format. Round-trips a parsed header up to
    /// the canonical lowercasing of CSV values and the loss of unknown
    /// formatting (the `Display` impl emits `key=val` pairs joined by `; `).
    #[must_use]
    pub fn to_header(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.containers.is_empty() {
            parts.push(format!("container={}", self.containers.join(",")));
        }
        if !self.video_decoders.is_empty() {
            parts.push(format!("vdec={}", self.video_decoders.join(",")));
        }
        if !self.audio_decoders.is_empty() {
            parts.push(format!("adec={}", self.audio_decoders.join(",")));
        }
        if !self.subtitles.is_empty() {
            parts.push(format!("subs={}", self.subtitles.join(",")));
        }
        if !self.hdr.is_empty() {
            parts.push(format!("hdr={}", self.hdr.join(",")));
        }
        push_bool(&mut parts, "webcodecs", self.webcodecs);
        push_bool(&mut parts, "webgpu", self.webgpu);
        push_bool(&mut parts, "mse", self.mse);
        push_bool(&mut parts, "mms", self.mms);
        push_bool(&mut parts, "legacy", self.legacy);
        if let Some(p) = &self.platform {
            parts.push(format!("platform={p}"));
        }
        if let Some(b) = &self.bandwidth {
            parts.push(format!("bandwidth={b}"));
        }
        for (k, v) in &self.extras {
            parts.push(format!("{k}={v}"));
        }
        parts.join("; ")
    }
}

fn csv_lower(s: &str) -> Vec<String> {
    s.split(',')
        .map(|p| p.trim().to_ascii_lowercase())
        .filter(|p| !p.is_empty())
        .collect()
}

fn parse_bool(s: &str) -> bool {
    matches!(
        s.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn push_bool(parts: &mut Vec<String>, name: &str, value: bool) {
    if value {
        parts.push(format!("{name}=1"));
    }
}

fn contains_ci(haystack: &[String], needle: &str) -> bool {
    let n = needle.to_ascii_lowercase();
    haystack.iter().any(|h| h.eq_ignore_ascii_case(&n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_typical_web_header() {
        let caps = ClientCapabilities::parse(
            "container=fmp4,mkv,webm; vdec=h264,hevc-hw,av1; adec=aac,opus,flac; \
             subs=webvtt,ass-overlay,pgs-overlay; webcodecs=1; webgpu=1; mse=1; mms=0; \
             hdr=hdr10,hlg; platform=web-chromium-134",
        );
        assert_eq!(caps.containers, ["fmp4", "mkv", "webm"]);
        assert_eq!(caps.video_decoders, ["h264", "hevc-hw", "av1"]);
        assert!(caps.webcodecs);
        assert!(caps.webgpu);
        assert!(caps.mse);
        assert!(!caps.mms);
        assert_eq!(caps.platform.as_deref(), Some("web-chromium-134"));
        assert!(caps.has_container("MKV"));
        assert!(caps.has_video_decoder("hevc-hw"));
    }

    #[test]
    fn preserves_unknown_keys() {
        let caps = ClientCapabilities::parse("future-codec=vvc; container=mp4");
        assert_eq!(caps.extras.get("future-codec").map(String::as_str), Some("vvc"));
        assert!(caps.has_container("mp4"));
    }

    #[test]
    fn ignores_malformed_pairs() {
        let caps = ClientCapabilities::parse(";;junk;=value;key=;container=mp4;");
        assert_eq!(caps.containers, ["mp4"]);
    }

    #[test]
    fn round_trips_through_header() {
        let original = "container=fmp4,mkv; vdec=h264,hevc-hw; webcodecs=1; platform=android-tv-14";
        let caps = ClientCapabilities::parse(original);
        let again = ClientCapabilities::parse(&caps.to_header());
        assert_eq!(caps.containers, again.containers);
        assert_eq!(caps.video_decoders, again.video_decoders);
        assert_eq!(caps.webcodecs, again.webcodecs);
        assert_eq!(caps.platform, again.platform);
    }

    #[test]
    fn boolean_variants() {
        for truthy in ["1", "true", "yes", "on", "TRUE", " 1 "] {
            let caps = ClientCapabilities::parse(&format!("webgpu={truthy}"));
            assert!(caps.webgpu, "expected truthy for {truthy:?}");
        }
        for falsy in ["0", "false", "no", "off", "", "garbage"] {
            let caps = ClientCapabilities::parse(&format!("webgpu={falsy}"));
            assert!(!caps.webgpu, "expected falsy for {falsy:?}");
        }
    }

    #[test]
    fn android_tv_caps() {
        let caps = ClientCapabilities::parse(
            "container=mkv,mp4,ts; vdec=h264,hevc,av1,vp9; adec=aac,ac3,eac3,truehd,dts,flac; \
             subs=webvtt,ass,pgs; platform=android-tv-14",
        );
        assert!(caps.has_audio_decoder("truehd"));
        assert!(caps.has_audio_decoder("DTS"));
        assert!(caps.supports_subtitle("pgs"));
    }
}
