//! Media probing + remux pipeline.
//!
//! - [`probe`] wraps `ffprobe -show_streams -show_format` and normalizes the
//!   output into a stable schema the API serves directly.
//! - [`remuxer`] runs `ffmpeg -c copy` once per source to produce a single
//!   CMAF-fragmented MP4 served via HTTP byte-range.
//! - [`subtitles`] converts text-based subtitle streams to `WebVTT` on the fly.

pub mod filename;
pub mod probe;
pub mod remuxer;
pub mod subtitles;

pub use probe::{
    AudioStream, MediaProbe, ProbeCache, ProbeError, SubtitleStream, VideoStream, probe_file,
};
pub use remuxer::{
    AudioCodec, AudioRendition, JobInfo as RemuxJobInfo, MASTER_PLAYLIST, RemuxError, RemuxManager,
    RemuxPlan,
};
pub use subtitles::SubtitleError;
pub use subtitles::cache_path as subtitle_cache_path;
pub use subtitles::stream_webvtt;
