//! `ffprobe` JSON wrapper.

use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

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
    /// Whether this stream's codec is browser-friendly (AAC / Opus / Vorbis /
    /// MP3). Other codecs require transcoding to AAC for HLS.
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
    /// Whether this stream can be losslessly converted to WebVTT (text-based
    /// subtitle codecs only).
    pub text_based: bool,
}

const TEXT_SUB_CODECS: &[&str] = &["subrip", "srt", "ass", "ssa", "mov_text", "webvtt"];
const BROWSER_AUDIO_CODECS: &[&str] = &["aac", "opus", "vorbis", "mp3"];

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
    let (mut vi, mut ai, mut si) = (0usize, 0usize, 0usize);
    for stream in raw.streams {
        match stream.codec_type.as_str() {
            "video" => {
                video.push(VideoStream {
                    index: vi,
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
                });
                vi += 1;
            }
            "audio" => {
                let codec = stream.codec_name.clone().unwrap_or_default();
                let lang = stream
                    .tags
                    .as_ref()
                    .and_then(|t| t.language.clone())
                    .or_else(|| {
                        stream.tags.as_ref().and_then(|t| t.lang.clone())
                    });
                let title = stream.tags.as_ref().and_then(|t| t.title.clone());
                let disp = stream.disposition.unwrap_or_default();
                audio.push(AudioStream {
                    index: ai,
                    absolute_index: stream.index,
                    browser_compatible: BROWSER_AUDIO_CODECS
                        .contains(&codec.to_ascii_lowercase().as_str()),
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
                });
                ai += 1;
            }
            "subtitle" => {
                let codec = stream.codec_name.clone().unwrap_or_default();
                let lang = stream
                    .tags
                    .as_ref()
                    .and_then(|t| t.language.clone())
                    .or_else(|| {
                        stream.tags.as_ref().and_then(|t| t.lang.clone())
                    });
                let title = stream.tags.as_ref().and_then(|t| t.title.clone());
                let disp = stream.disposition.unwrap_or_default();
                subtitle.push(SubtitleStream {
                    index: si,
                    absolute_index: stream.index,
                    text_based: TEXT_SUB_CODECS
                        .contains(&codec.to_ascii_lowercase().as_str()),
                    codec,
                    language: lang,
                    title,
                    default: disp.default == 1,
                    forced: disp.forced == 1,
                });
                si += 1;
            }
            _ => {}
        }
    }

    MediaProbe {
        container: format_name,
        duration_seconds: duration,
        size_bytes: size,
        bit_rate,
        video,
        audio,
        subtitle,
    }
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
