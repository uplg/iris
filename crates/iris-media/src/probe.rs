//! `ffprobe` JSON wrapper + in-memory cache.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug, Error)]
pub enum ProbeError {
    #[error("ffprobe failed (status {0}): {1}")]
    Failed(i32, String),
    #[error("ffprobe output not parseable: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize)]
pub struct MediaProbe {
    pub container: String,
    pub duration_seconds: Option<f64>,
    pub size_bytes: Option<u64>,
    pub bit_rate: Option<u64>,
    pub video: Vec<VideoStream>,
    pub audio: Vec<AudioStream>,
    pub subtitle: Vec<SubtitleStream>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VideoStream {
    /// Index within video streams (0-based, NOT the global stream index).
    pub index: usize,
    pub absolute_index: u32,
    pub codec: String,
    pub profile: Option<String>,
    /// Codec level encoded as the integer ffprobe reports (e.g. 51 = level 5.1).
    pub level: Option<u32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bit_rate: Option<u64>,
    pub frame_rate: Option<f64>,
    /// Frame rate as a rational, when available. The legacy `frame_rate` float
    /// remains for existing callers; this field gives the exact ratio MSE
    /// timestamps need.
    pub frame_rate_num: Option<u32>,
    pub frame_rate_den: Option<u32>,
    pub bit_depth: Option<u8>,
    pub color_primaries: Option<String>,
    pub color_transfer: Option<String>,
    pub color_space: Option<String>,
    /// Detected HDR flavour, derived from `color_transfer` + side data.
    pub hdr: HdrKind,
    pub max_cll: Option<u32>,
    pub max_fall: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HdrKind {
    None,
    Hdr10,
    Hdr10Plus,
    Dovi,
    Hlg,
}

#[derive(Debug, Clone, Serialize)]
pub struct AudioStream {
    pub index: usize,
    pub absolute_index: u32,
    pub codec: String,
    pub channels: u32,
    pub channel_layout: Option<String>,
    pub sample_rate: Option<u32>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub default: bool,
    pub forced: bool,
    /// Whether this stream's codec is one most browsers can decode natively
    /// (AAC / Opus / Vorbis / MP3). Other codecs (DTS, AC-3, EAC-3, FLAC)
    /// are passed through untouched in the remuxed fMP4 — playback works
    /// where the OS/browser supports them, and stays silent otherwise.
    pub browser_compatible: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubtitleStream {
    pub index: usize,
    pub absolute_index: u32,
    pub codec: String,
    pub language: Option<String>,
    pub title: Option<String>,
    pub default: bool,
    pub forced: bool,
    /// Whether this stream can be losslessly converted to `WebVTT`
    /// (text-based subtitle codecs only).
    pub text_based: bool,
}

const TEXT_SUB_CODECS: &[&str] = &["subrip", "srt", "ass", "ssa", "mov_text", "webvtt"];
const BROWSER_AUDIO_CODECS: &[&str] = &["aac", "opus", "vorbis", "mp3"];

/// In-memory probe cache. Keyed by `(infohash, file_idx)`. Probe is
/// expensive on multi-GB MKVs (ffmpeg has to read the index, which often
/// lives at the end of the file), so we keep results around for the lifetime
/// of the process. Invalidate explicitly when the underlying file is
/// replaced (e.g. after GC eviction + re-ingest).
#[derive(Clone, Default)]
pub struct ProbeCache {
    inner: Arc<RwLock<HashMap<String, MediaProbe>>>,
}

impl ProbeCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn get_or_probe(
        &self,
        infohash: &str,
        file_idx: usize,
        path: &Path,
    ) -> Result<MediaProbe, ProbeError> {
        let key = format!("{infohash}:{file_idx}");
        if let Some(hit) = self.inner.read().await.get(&key).cloned() {
            return Ok(hit);
        }
        let probe = probe_file(path).await?;
        self.inner.write().await.insert(key, probe.clone());
        Ok(probe)
    }

    pub async fn invalidate(&self, infohash: &str) {
        let prefix = format!("{infohash}:");
        self.inner
            .write()
            .await
            .retain(|k, _| !k.starts_with(&prefix));
    }
}

pub async fn probe_file(path: &Path) -> Result<MediaProbe, ProbeError> {
    // Pre-flight: surface "file isn't on disk yet" with a recognisable
    // message instead of letting ffprobe choke on it. The frontend's
    // probe-retry policy keys on this exact substring to keep polling
    // while the torrent finishes downloading.
    match tokio::fs::metadata(path).await {
        Ok(m) if m.len() == 0 => {
            return Err(ProbeError::Failed(
                -1,
                format!("file not yet on disk (zero bytes at {})", path.display()),
            ));
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(ProbeError::Failed(
                -1,
                format!("file not yet on disk ({})", path.display()),
            ));
        }
        Err(e) => {
            return Err(ProbeError::Failed(
                -1,
                format!("cannot stat {}: {e}", path.display()),
            ));
        }
    }

    let output = tokio::process::Command::new("ffprobe")
        .args([
            // `-v error` surfaces ffprobe's own error messages on stderr.
            // The previous `-v quiet` swallowed them, so a failed probe
            // returned `ffprobe failed (status 1):` with no clue what
            // went wrong.
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let msg = if stderr.is_empty() {
            format!("ffprobe exited {} with no stderr (path: {})",
                output.status.code().unwrap_or(-1),
                path.display())
        } else {
            stderr
        };
        return Err(ProbeError::Failed(
            output.status.code().unwrap_or(-1),
            msg,
        ));
    }

    let raw: FfprobeOutput = serde_json::from_slice(&output.stdout)?;
    Ok(normalize(raw))
}

fn normalize(raw: FfprobeOutput) -> MediaProbe {
    let format_name = raw
        .format
        .as_ref()
        .map(|f| f.format_name.clone())
        .unwrap_or_default();
    let duration = raw
        .format
        .as_ref()
        .and_then(|f| f.duration.as_ref())
        .and_then(|s| s.parse::<f64>().ok());
    let size = raw
        .format
        .as_ref()
        .and_then(|f| f.size.as_ref())
        .and_then(|s| s.parse::<u64>().ok());
    let bit_rate = raw
        .format
        .as_ref()
        .and_then(|f| f.bit_rate.as_ref())
        .and_then(|s| s.parse::<u64>().ok());

    let mut video = Vec::new();
    let mut audio = Vec::new();
    let mut subtitle = Vec::new();
    for stream in raw.streams {
        match stream.codec_type.as_str() {
            "video" => video.push(normalize_video(stream, video.len())),
            "audio" => audio.push(normalize_audio(stream, audio.len())),
            "subtitle" => subtitle.push(normalize_subtitle(stream, subtitle.len())),
            _ => {}
        }
    }

    // Some MKVs in the wild concatenate the same program twice — the
    // probe ends up with N copies of identical (codec, language, title)
    // streams. Surfacing them all means the audio menu shows "Japanese"
    // twice, the subtitle list shows "French" twice, and the remux plan
    // builds a duplicate audio rendition. Drop dupes here, keeping the
    // first occurrence (its `absolute_index` is what ffmpeg gets pointed
    // at) and renumbering the surviving entries densely.
    MediaProbe {
        container: format_name,
        duration_seconds: duration,
        size_bytes: size,
        bit_rate,
        video: dedupe_video(video),
        audio: dedupe_audio(audio),
        subtitle: dedupe_subtitles(subtitle),
    }
}

fn dedupe_video(items: Vec<VideoStream>) -> Vec<VideoStream> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(items.len());
    for mut s in items {
        let key = (
            s.codec.clone(),
            s.profile.clone(),
            s.width,
            s.height,
            s.bit_depth,
            // Quantise the frame rate to mil­li-fps (24.0 → 24000) so the
            // tuple's `Eq` works on `f64` indirectly via i64. Real frame
            // rates are 12–120 fps, can't overflow i64.
            #[allow(clippy::cast_possible_truncation)]
            s.frame_rate.map(|f| (f * 1000.0).round() as i64),
        );
        if !seen.insert(key) {
            continue;
        }
        s.index = out.len();
        out.push(s);
    }
    out
}

fn dedupe_audio(items: Vec<AudioStream>) -> Vec<AudioStream> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(items.len());
    for mut s in items {
        let key = (
            s.codec.clone(),
            s.language.clone(),
            s.title.clone(),
            s.channels,
            s.default,
            s.forced,
        );
        if !seen.insert(key) {
            continue;
        }
        s.index = out.len();
        out.push(s);
    }
    out
}

fn dedupe_subtitles(items: Vec<SubtitleStream>) -> Vec<SubtitleStream> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(items.len());
    for mut s in items {
        let key = (
            s.codec.clone(),
            s.language.clone(),
            s.title.clone(),
            s.default,
            s.forced,
        );
        if !seen.insert(key) {
            continue;
        }
        s.index = out.len();
        out.push(s);
    }
    out
}

fn normalize_video(stream: RawStream, index: usize) -> VideoStream {
    let (fps_num, fps_den) = parse_frame_rate_rational(stream.avg_frame_rate.as_deref());
    let frame_rate = match (fps_num, fps_den) {
        (Some(n), Some(d)) if d != 0 => Some(f64::from(n) / f64::from(d)),
        _ => None,
    };
    let bit_depth = stream
        .bits_per_raw_sample
        .as_ref()
        .and_then(|s| s.parse::<u8>().ok())
        .or_else(|| bit_depth_from_pix_fmt(stream.pix_fmt.as_deref()));
    let hdr = detect_hdr(
        stream.color_transfer.as_deref(),
        stream.side_data_list.as_deref(),
    );
    let (max_cll, max_fall) = content_light_level(stream.side_data_list.as_deref());
    VideoStream {
        index,
        absolute_index: stream.index,
        codec: stream.codec_name.unwrap_or_default(),
        profile: stream.profile,
        level: stream.level.and_then(|l| u32::try_from(l).ok()),
        width: stream.width,
        height: stream.height,
        bit_rate: stream
            .bit_rate
            .as_ref()
            .and_then(|s| s.parse::<u64>().ok()),
        frame_rate,
        frame_rate_num: fps_num,
        frame_rate_den: fps_den,
        bit_depth,
        color_primaries: stream.color_primaries,
        color_transfer: stream.color_transfer,
        color_space: stream.color_space,
        hdr,
        max_cll,
        max_fall,
    }
}

fn normalize_audio(stream: RawStream, index: usize) -> AudioStream {
    let codec = stream.codec_name.clone().unwrap_or_default();
    let (lang, title) = lang_and_title(stream.tags.as_ref());
    let disp = stream.disposition.unwrap_or_default();
    AudioStream {
        index,
        absolute_index: stream.index,
        browser_compatible: BROWSER_AUDIO_CODECS.contains(&codec.to_ascii_lowercase().as_str()),
        codec,
        channels: stream.channels.unwrap_or_default(),
        channel_layout: stream.channel_layout,
        sample_rate: stream
            .sample_rate
            .as_ref()
            .and_then(|s| s.parse::<u32>().ok()),
        language: lang,
        title,
        default: disp.default == 1,
        forced: disp.forced == 1,
    }
}

fn normalize_subtitle(stream: RawStream, index: usize) -> SubtitleStream {
    let codec = stream.codec_name.clone().unwrap_or_default();
    let (lang, title) = lang_and_title(stream.tags.as_ref());
    let disp = stream.disposition.unwrap_or_default();
    SubtitleStream {
        index,
        absolute_index: stream.index,
        text_based: TEXT_SUB_CODECS.contains(&codec.to_ascii_lowercase().as_str()),
        codec,
        language: lang,
        title,
        default: disp.default == 1,
        forced: disp.forced == 1,
    }
}

fn lang_and_title(tags: Option<&RawTags>) -> (Option<String>, Option<String>) {
    let lang = tags
        .and_then(|t| t.language.clone())
        .or_else(|| tags.and_then(|t| t.lang.clone()));
    let title = tags.and_then(|t| t.title.clone());
    (lang, title)
}

fn parse_frame_rate_rational(s: Option<&str>) -> (Option<u32>, Option<u32>) {
    let Some(s) = s else { return (None, None); };
    if let Some((a, b)) = s.split_once('/') {
        let num = a.parse::<u32>().ok();
        let den = b.parse::<u32>().ok();
        if matches!(den, Some(0)) {
            return (None, None);
        }
        return (num, den);
    }
    // Single number e.g. "23.976" — convert to milli-fps rational.
    if let Ok(f) = s.parse::<f64>() {
        if f > 0.0 && f < 1000.0 {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let num = (f * 1000.0).round() as u32;
            return (Some(num), Some(1000));
        }
    }
    (None, None)
}

fn bit_depth_from_pix_fmt(pix_fmt: Option<&str>) -> Option<u8> {
    let p = pix_fmt?.to_ascii_lowercase();
    // ffmpeg pix_fmt names embed the bit depth: yuv420p10le, p010le, gbrp12le…
    for depth in [16_u8, 14, 12, 10] {
        if p.contains(&format!("p{depth}")) || p.contains(&format!("{depth}le")) {
            return Some(depth);
        }
    }
    if p.starts_with("yuv") || p.starts_with("nv") || p == "gbrp" {
        return Some(8);
    }
    None
}

fn detect_hdr(color_transfer: Option<&str>, side_data: Option<&[RawSideData]>) -> HdrKind {
    let has_dovi = side_data.is_some_and(|sd| sd.iter().any(RawSideData::is_dovi));
    if has_dovi {
        return HdrKind::Dovi;
    }
    let has_hdr10plus = side_data.is_some_and(|sd| sd.iter().any(RawSideData::is_hdr10plus));
    match color_transfer.map(str::to_ascii_lowercase).as_deref() {
        Some("smpte2084") if has_hdr10plus => HdrKind::Hdr10Plus,
        Some("smpte2084") => HdrKind::Hdr10,
        Some("arib-std-b67") => HdrKind::Hlg,
        _ => HdrKind::None,
    }
}

fn content_light_level(side_data: Option<&[RawSideData]>) -> (Option<u32>, Option<u32>) {
    let Some(items) = side_data else { return (None, None); };
    for item in items {
        if item.is_content_light_level() {
            return (item.max_content, item.max_average);
        }
    }
    (None, None)
}

// ---- raw ffprobe schema (only the fields we actually consume) ----

#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<RawStream>,
    format: Option<RawFormat>,
}

#[derive(Debug, Deserialize)]
struct RawStream {
    index: u32,
    codec_type: String,
    codec_name: Option<String>,
    profile: Option<String>,
    level: Option<i32>,
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    channels: Option<u32>,
    channel_layout: Option<String>,
    sample_rate: Option<String>,
    bit_rate: Option<String>,
    bits_per_raw_sample: Option<String>,
    pix_fmt: Option<String>,
    color_primaries: Option<String>,
    color_transfer: Option<String>,
    color_space: Option<String>,
    side_data_list: Option<Vec<RawSideData>>,
    tags: Option<RawTags>,
    disposition: Option<RawDisposition>,
}

#[derive(Debug, Deserialize)]
struct RawSideData {
    side_data_type: Option<String>,
    /// Present on `"Content light level metadata"` entries.
    #[serde(default)]
    max_content: Option<u32>,
    /// Present on `"Content light level metadata"` entries.
    #[serde(default)]
    max_average: Option<u32>,
}

impl RawSideData {
    fn type_lower(&self) -> Option<String> {
        self.side_data_type
            .as_ref()
            .map(|s| s.to_ascii_lowercase())
    }

    fn is_dovi(&self) -> bool {
        // ffprobe labels it "DOVI configuration record".
        self.type_lower()
            .is_some_and(|t| t.contains("dovi") || t.contains("dolby vision"))
    }

    fn is_hdr10plus(&self) -> bool {
        self.type_lower()
            .is_some_and(|t| t.contains("hdr10+") || t.contains("hdr dynamic metadata"))
    }

    fn is_content_light_level(&self) -> bool {
        self.type_lower()
            .is_some_and(|t| t.contains("content light level"))
    }
}

#[derive(Debug, Deserialize)]
struct RawTags {
    language: Option<String>,
    #[serde(rename = "LANGUAGE")]
    lang: Option<String>,
    title: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawDisposition {
    #[serde(default)]
    default: u8,
    #[serde(default)]
    forced: u8,
}

#[derive(Debug, Deserialize)]
struct RawFormat {
    format_name: String,
    duration: Option<String>,
    size: Option<String>,
    bit_rate: Option<String>,
}
