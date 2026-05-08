//! Media probing + HLS segmentation pipeline.
//!
//! - [`probe`] wraps `ffprobe -show_streams -show_format` and normalizes the
//!   output into a stable schema the API serves directly.
//! - [`hls`] manages on-demand HLS segmentation: it spawns `ffmpeg` once per
//!   `(file, audio_track)` pair, writes fMP4 segments to disk, and lets the
//!   HTTP layer serve them as static files.
//! - [`subtitles`] converts text-based subtitle streams to WebVTT on the fly.

pub mod hls;
pub mod probe;
pub mod subtitles;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackMode {
    Direct,
    Remux,
    Transcode,
}

pub use hls::{HlsError, HlsManager};
pub use probe::{
    AudioStream, MediaProbe, ProbeError, SubtitleStream, VideoStream, probe_file,
};
pub use subtitles::SubtitleError;
pub use subtitles::cache_path as subtitle_cache_path;
pub use subtitles::stream_webvtt;
