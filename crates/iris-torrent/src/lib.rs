//! Torrent engine wrapper.
//!
//! - `announce` sends the `event=stopped` tracker announce librqbit omits.
//! - [`metadata`] parses `.torrent` bytes for preview (no network activity).
//! - [`engine`] wraps a [`librqbit::Session`] and exposes a small,
//!   iris-friendly API for ingestion, listing, deletion and streaming.

mod announce;
pub mod engine;
pub mod gc;
pub mod metadata;

pub use engine::{Engine, EngineError, FileEntry, IngestResult, TorrentSnapshot, TorrentState};
pub use gc::{DerivedCache, DerivedTrimFn, Gc, GcConfig, GcReport};
pub use metadata::{TorrentFilePreview, TorrentPreview, parse_preview};
