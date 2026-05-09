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
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bit_rate: Option<u64>,
    pub frame_rate: Option<f64>,
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
    let output = tokio::process::Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output()
        .await?;

    if !output.status.success() {
        return Err(ProbeError::Failed(
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).into_owned(),
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
    VideoStream {
        index,
        absolute_index: stream.index,
        codec: stream.codec_name.unwrap_or_default(),
        profile: stream.profile,
        width: stream.width,
        height: stream.height,
        bit_rate: stream
            .bit_rate
            .as_ref()
            .and_then(|s| s.parse::<u64>().ok()),
        frame_rate: parse_frame_rate(stream.avg_frame_rate.as_deref()),
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

fn parse_frame_rate(s: Option<&str>) -> Option<f64> {
    let s = s?;
    if let Some((a, b)) = s.split_once('/') {
        let a: f64 = a.parse().ok()?;
        let b: f64 = b.parse().ok()?;
        if b == 0.0 {
            return None;
        }
        return Some(a / b);
    }
    s.parse().ok()
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
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    channels: Option<u32>,
    channel_layout: Option<String>,
    sample_rate: Option<String>,
    bit_rate: Option<String>,
    tags: Option<RawTags>,
    disposition: Option<RawDisposition>,
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
