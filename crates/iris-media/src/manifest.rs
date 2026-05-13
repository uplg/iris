//! Playback manifest — the per-file contract Iris clients fetch before
//! deciding how to play a torrent file.
//!
//! Built on top of [`crate::probe::MediaProbe`], the manifest adds the
//! information clients need to pick a decode tier without round-tripping:
//!
//! - MSE / `WebCodecs` codec strings for `isTypeSupported` / `isConfigSupported`
//! - HDR transfer + colour metadata
//! - Container index location (so the client and the sparse-streaming layer
//!   can prefetch the right byte range)
//! - Download progress + complete byte ranges
//! - Sidecar URLs for extracted subtitle tracks
//!
//! See `docs/SOTA_ARCHITECTURE.md` §2.1 for the wire format.
//!
//! Container layout detection is best-effort. Phase 0 ships a conservative
//! detector that recognises MP4 `moov`-at-start by reading the first 16
//! bytes; everything else is reported as `index_at_end = true`, which is the
//! safe default for the Phase 1 tail-prefetch optimiser.

use std::path::Path;

use serde::Serialize;

use crate::probe::{AudioStream, HdrKind, MediaProbe, SubtitleStream, VideoStream};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub infohash: String,
    pub file_idx: u32,
    pub filename: String,

    pub container: String,
    pub duration_s: Option<f64>,
    pub size_bytes: u64,

    /// `Some(true)` if a fast-start MP4 with `moov` ahead of `mdat`,
    /// `Some(false)` if MP4 with trailing `moov`, `None` for non-MP4.
    pub moov_at_start: Option<bool>,
    /// `true` if the demux index lives near the end of the file
    /// (MKV Cues, MP4 trailing `moov`, AVI `idx1`). The Phase 1 sparse
    /// streamer uses this to prioritise tail bytes on first read.
    pub index_at_end: bool,
    pub header_byte_range: ByteRange,
    pub tail_byte_range: Option<ByteRange>,

    pub download: DownloadStatus,

    pub video: Vec<VideoTrack>,
    pub audio: Vec<AudioTrack>,
    pub subtitles: Vec<SubtitleTrack>,
    pub chapters: Vec<Chapter>,
}

/// Inclusive byte range `[start, end]`, matching HTTP `Range:` semantics.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DownloadStatus {
    pub progress: f64,
    pub ranges_complete: Vec<[u64; 2]>,
    pub bytes_complete: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct VideoTrack {
    pub stream_idx: u32,
    pub codec: String,
    /// MSE / `WebCodecs` codec string when we can produce one with
    /// confidence (e.g. `"avc1.640028"`, `"hev1.2.4.L153.B0"`).
    pub codec_string: Option<String>,
    pub profile: Option<String>,
    pub level: Option<u32>,
    pub bit_depth: Option<u8>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps_num: Option<u32>,
    pub fps_den: Option<u32>,
    pub hdr: HdrKind,
    pub color_primaries: Option<String>,
    pub color_transfer: Option<String>,
    pub color_matrix: Option<String>,
    pub max_cll: Option<u32>,
    pub max_fall: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AudioTrack {
    pub stream_idx: u32,
    pub codec: String,
    pub codec_string: Option<String>,
    pub channels: u32,
    pub channel_layout: Option<String>,
    pub sample_rate: Option<u32>,
    pub bitrate: Option<u64>,
    pub lang: Option<String>,
    pub title: Option<String>,
    pub default: bool,
    pub forced: bool,
    /// Whether the codec is one every modern browser decodes natively.
    /// Re-exposed from the probe's `browser_compatible` flag.
    pub browser_native: bool,
}

#[derive(Debug, Clone, Serialize)]
#[allow(clippy::struct_excessive_bools)] // schema-driven; these flags are part of the wire format
pub struct SubtitleTrack {
    pub stream_idx: u32,
    pub codec: String,
    pub lang: Option<String>,
    pub title: Option<String>,
    pub default: bool,
    pub forced: bool,
    pub text_based: bool,
    /// Always true today — kept as an explicit field so future codecs that
    /// the server can't demux can be marked `false` without breaking the
    /// schema.
    pub extractable: bool,
    /// Relative URL the client should GET to receive the extracted track.
    /// Suffix is `.vtt` for text subs, `.sup` for PGS, `.ass` for ASS/SSA.
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Chapter {
    pub start_s: f64,
    pub end_s: f64,
    pub title: Option<String>,
}

/// Inputs the API layer passes in; everything the manifest needs that
/// doesn't come from the probe.
pub struct ManifestInputs<'a> {
    pub infohash: &'a str,
    pub file_idx: u32,
    pub filename: &'a str,
    pub size_bytes: u64,
    pub download_progress: f64,
    pub ranges_complete: Vec<(u64, u64)>,
    pub bytes_complete: u64,
}

/// Build a manifest from a probe + caller-supplied inputs. Reads the first
/// 16 bytes of `file_path` (when provided) to detect MP4 `moov` placement.
pub async fn build(
    probe: &MediaProbe,
    inputs: ManifestInputs<'_>,
    file_path: Option<&Path>,
) -> Manifest {
    let layout = match file_path {
        Some(p) => detect_layout(&probe.container, p, inputs.size_bytes).await,
        None => ContainerLayout::default_for(&probe.container, inputs.size_bytes),
    };

    Manifest {
        schema_version: SCHEMA_VERSION,
        infohash: inputs.infohash.to_owned(),
        file_idx: inputs.file_idx,
        filename: inputs.filename.to_owned(),
        container: probe.container.clone(),
        duration_s: probe.duration_seconds,
        size_bytes: inputs.size_bytes,
        moov_at_start: layout.moov_at_start,
        index_at_end: layout.index_at_end,
        header_byte_range: layout.header_byte_range,
        tail_byte_range: layout.tail_byte_range,
        download: DownloadStatus {
            progress: inputs.download_progress,
            ranges_complete: inputs
                .ranges_complete
                .into_iter()
                .map(|(s, e)| [s, e])
                .collect(),
            bytes_complete: inputs.bytes_complete,
        },
        video: probe.video.iter().map(video_track).collect(),
        audio: probe.audio.iter().map(audio_track).collect(),
        subtitles: probe
            .subtitle
            .iter()
            .map(|s| subtitle_track(s, inputs.infohash, inputs.file_idx))
            .collect(),
        chapters: Vec::new(),
    }
}

fn video_track(v: &VideoStream) -> VideoTrack {
    VideoTrack {
        stream_idx: v.absolute_index,
        codec_string: codec_string_video(v),
        codec: v.codec.clone(),
        profile: v.profile.clone(),
        level: v.level,
        bit_depth: v.bit_depth,
        width: v.width,
        height: v.height,
        fps_num: v.frame_rate_num,
        fps_den: v.frame_rate_den,
        hdr: v.hdr,
        color_primaries: v.color_primaries.clone(),
        color_transfer: v.color_transfer.clone(),
        color_matrix: v.color_space.clone(),
        max_cll: v.max_cll,
        max_fall: v.max_fall,
    }
}

fn audio_track(a: &AudioStream) -> AudioTrack {
    AudioTrack {
        stream_idx: a.absolute_index,
        codec_string: codec_string_audio(&a.codec),
        codec: a.codec.clone(),
        channels: a.channels,
        channel_layout: a.channel_layout.clone(),
        sample_rate: a.sample_rate,
        bitrate: None,
        lang: a.language.clone(),
        title: a.title.clone(),
        default: a.default,
        forced: a.forced,
        browser_native: a.browser_compatible,
    }
}

fn subtitle_track(s: &SubtitleStream, infohash: &str, file_idx: u32) -> SubtitleTrack {
    let ext = subtitle_url_ext(&s.codec);
    let url = format!(
        "/api/torrents/{infohash}/files/{file_idx}/sub/{stream_idx}/track.{ext}",
        stream_idx = s.absolute_index
    );
    SubtitleTrack {
        stream_idx: s.absolute_index,
        codec: s.codec.clone(),
        lang: s.language.clone(),
        title: s.title.clone(),
        default: s.default,
        forced: s.forced,
        text_based: s.text_based,
        extractable: true,
        url,
    }
}

fn subtitle_url_ext(codec: &str) -> &'static str {
    match codec.to_ascii_lowercase().as_str() {
        "pgs" | "hdmv_pgs_subtitle" | "dvb_subtitle" | "dvd_subtitle" => "sup",
        "ass" | "ssa" => "ass",
        _ => "vtt",
    }
}

// ---- codec strings (MSE / WebCodecs format, best-effort) ----

fn codec_string_video(v: &VideoStream) -> Option<String> {
    match v.codec.to_ascii_lowercase().as_str() {
        "h264" | "avc" | "avc1" => avc1_codec_string(v),
        "hevc" | "h265" | "hev1" | "hvc1" => hevc_codec_string(v),
        "av1" => av1_codec_string(v),
        "vp9" => vp9_codec_string(v),
        _ => None,
    }
}

fn avc1_codec_string(v: &VideoStream) -> Option<String> {
    // avc1.PPCCLL — profile_idc, constraint_set_flags, level_idc as 6 hex chars.
    // Without the SPS we approximate from `profile` + `level`.
    let profile_idc: u8 = match v.profile.as_deref().map(str::to_ascii_lowercase).as_deref() {
        Some("baseline" | "constrained baseline") => 0x42,
        Some("main") => 0x4D,
        Some("extended") => 0x58,
        Some("high") => 0x64,
        Some("high 10") => 0x6E,
        Some("high 4:2:2") => 0x7A,
        Some("high 4:4:4 predictive") => 0xF4,
        _ => return None,
    };
    let level = v.level?;
    if level > 255 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    let level_byte = level as u8;
    Some(format!("avc1.{profile_idc:02X}00{level_byte:02X}"))
}

fn hevc_codec_string(v: &VideoStream) -> Option<String> {
    // hev1.<profile_space>.<profile_idc>.<tier_flag>L<level>.B0
    // Profile space is 0 for all common variants.
    let profile_idc: u8 = match v.profile.as_deref().map(str::to_ascii_lowercase).as_deref() {
        Some("main") => 1,
        Some("main 10") => 2,
        Some("main still picture") => 3,
        Some("rext" | "range extensions") => 4,
        _ => return None,
    };
    let level = v.level?;
    Some(format!("hev1.{profile_idc}.4.L{level}.B0"))
}

fn av1_codec_string(v: &VideoStream) -> Option<String> {
    // av01.<profile>.<level><tier>.<bit_depth>
    let profile: u8 = match v.profile.as_deref().map(str::to_ascii_lowercase).as_deref() {
        Some("main") | None => 0,
        Some("high") => 1,
        Some("professional") => 2,
        _ => return None,
    };
    let level = v.level?;
    let bit_depth = v.bit_depth.unwrap_or(8);
    Some(format!("av01.{profile}.{level:02}M.{bit_depth:02}"))
}

fn vp9_codec_string(v: &VideoStream) -> Option<String> {
    // vp09.<profile>.<level>.<bit_depth>
    let profile: u8 = match v.profile.as_deref().map(str::to_ascii_lowercase).as_deref() {
        Some("profile 0" | "0") | None => 0,
        Some("profile 1" | "1") => 1,
        Some("profile 2" | "2") => 2,
        Some("profile 3" | "3") => 3,
        _ => return None,
    };
    let level = v.level?;
    let bit_depth = v.bit_depth.unwrap_or(8);
    Some(format!("vp09.{profile:02}.{level:02}.{bit_depth:02}"))
}

fn codec_string_audio(codec: &str) -> Option<String> {
    match codec.to_ascii_lowercase().as_str() {
        "aac" => Some("mp4a.40.2".to_owned()),
        "mp3" => Some("mp4a.40.34".to_owned()),
        "opus" => Some("opus".to_owned()),
        "vorbis" => Some("vorbis".to_owned()),
        "flac" => Some("flac".to_owned()),
        "ac3" => Some("ac-3".to_owned()),
        "eac3" => Some("ec-3".to_owned()),
        _ => None,
    }
}

// ---- container layout detection ----

#[derive(Debug, Clone, Copy)]
struct ContainerLayout {
    moov_at_start: Option<bool>,
    index_at_end: bool,
    header_byte_range: ByteRange,
    tail_byte_range: Option<ByteRange>,
}

impl ContainerLayout {
    fn default_for(_container: &str, size_bytes: u64) -> Self {
        // Conservative default: assume the index lives at the end of the
        // file. The Phase 1 streamer prefetches the tail on first read,
        // which is correct for MKV Cues / MP4 trailing `moov` / AVI `idx1`
        // / unknown formats. MP4 fast-start gets corrected in
        // `detect_layout` when we can sniff the first 16 bytes.
        let head_end = 8192_u64.min(size_bytes.saturating_sub(1));
        Self {
            moov_at_start: None,
            index_at_end: true,
            header_byte_range: ByteRange {
                start: 0,
                end: head_end,
            },
            tail_byte_range: Some(tail_range(size_bytes)),
        }
    }
}

fn tail_range(size_bytes: u64) -> ByteRange {
    // Read enough to catch a typical MKV Cues block or trailing moov.
    // 1 MiB is plenty for files up to ~50 GB; we cap and don't go negative.
    let window: u64 = 1 << 20;
    let end = size_bytes.saturating_sub(1);
    let start = size_bytes.saturating_sub(window);
    ByteRange { start, end }
}

#[derive(Debug, Clone, Copy)]
enum ContainerKind {
    Mp4,
    Matroska,
    Avi,
    Other,
}

fn container_kind(container: &str) -> ContainerKind {
    let c = container.to_ascii_lowercase();
    if c.contains("matroska") || c.contains("webm") {
        ContainerKind::Matroska
    } else if c.contains("mp4") || c.contains("mov") || c.contains("m4v") || c.contains("3gp") {
        ContainerKind::Mp4
    } else if c.contains("avi") {
        ContainerKind::Avi
    } else {
        ContainerKind::Other
    }
}

async fn detect_layout(container: &str, path: &Path, size_bytes: u64) -> ContainerLayout {
    let mut layout = ContainerLayout::default_for(container, size_bytes);
    if matches!(container_kind(container), ContainerKind::Mp4) {
        // Read the first 16 bytes: <size:4><type:4><size:4><type:4>. If the
        // second box type is `moov`, the file is fast-start and a streaming
        // client can begin demuxing from byte 0 without prefetching the tail.
        if let Some(moov_first) = read_mp4_moov_at_start(path).await {
            layout.moov_at_start = Some(moov_first);
            if moov_first {
                layout.index_at_end = false;
                layout.tail_byte_range = None;
            }
        }
    }
    layout
}

async fn read_mp4_moov_at_start(path: &Path) -> Option<bool> {
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(path).await.ok()?;
    let mut buf = [0_u8; 32];
    let n = file.read(&mut buf).await.ok()?;
    if n < 16 {
        return None;
    }
    // First box must be ftyp for an MP4; if not, this isn't actually MP4.
    if &buf[4..8] != b"ftyp" {
        return None;
    }
    let first_box_size = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if first_box_size < 8 || first_box_size > n.saturating_sub(8) {
        return None;
    }
    let next_type = &buf[first_box_size + 4..first_box_size + 8];
    Some(next_type == b"moov")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vp9_codec_string_main_profile() {
        let v = make_video("vp9", Some("Profile 0"), Some(40), Some(8));
        assert_eq!(codec_string_video(&v).as_deref(), Some("vp09.00.40.08"));
    }

    #[test]
    fn hevc_main10_l51() {
        let v = make_video("hevc", Some("Main 10"), Some(153), Some(10));
        assert_eq!(codec_string_video(&v).as_deref(), Some("hev1.2.4.L153.B0"));
    }

    #[test]
    fn avc1_high_l40() {
        let v = make_video("h264", Some("High"), Some(40), Some(8));
        assert_eq!(codec_string_video(&v).as_deref(), Some("avc1.640028"));
    }

    #[test]
    fn av1_main_l8_10bit() {
        let v = make_video("av1", Some("Main"), Some(8), Some(10));
        assert_eq!(codec_string_video(&v).as_deref(), Some("av01.0.08M.10"));
    }

    #[test]
    fn unknown_codec_returns_none() {
        let v = make_video("vc1", None, None, None);
        assert!(codec_string_video(&v).is_none());
    }

    fn make_video(
        codec: &str,
        profile: Option<&str>,
        level: Option<u32>,
        bit_depth: Option<u8>,
    ) -> VideoStream {
        VideoStream {
            index: 0,
            absolute_index: 0,
            codec: codec.to_owned(),
            profile: profile.map(str::to_owned),
            level,
            width: None,
            height: None,
            bit_rate: None,
            frame_rate: None,
            frame_rate_num: None,
            frame_rate_den: None,
            bit_depth,
            color_primaries: None,
            color_transfer: None,
            color_space: None,
            hdr: HdrKind::None,
            max_cll: None,
            max_fall: None,
        }
    }
}
