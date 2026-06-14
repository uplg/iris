//! Pure metadata parsing — no network activity.

use librqbit::{ByteBuf, TorrentMetaV1, torrent_from_bytes};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TorrentPreview {
    pub infohash: String,
    pub name: String,
    pub total_size_bytes: u64,
    pub piece_length: u32,
    pub piece_count: u32,
    pub announce_urls: Vec<String>,
    pub files: Vec<TorrentFilePreview>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TorrentFilePreview {
    pub index: usize,
    pub path: String,
    pub size_bytes: u64,
    /// Lowercase extension (no dot), e.g. "mkv", "mp4", "srt".
    pub extension: Option<String>,
    /// Heuristic: `true` when the extension is a typical video container.
    pub is_video: bool,
}

const VIDEO_EXTS: &[&str] = &[
    "mkv", "mp4", "m4v", "avi", "mov", "webm", "ts", "mts", "m2ts", "wmv",
];

pub fn parse_preview(bytes: &[u8]) -> anyhow::Result<TorrentPreview> {
    let parsed: TorrentMetaV1<ByteBuf> = torrent_from_bytes(bytes)?;
    let info_hash = hex::encode(parsed.info_hash.0);

    let mut announce_urls = Vec::new();
    if let Some(a) = parsed.announce.as_ref()
        && let Ok(s) = std::str::from_utf8(a.as_ref())
    {
        announce_urls.push(s.to_string());
    }
    for tier in &parsed.announce_list {
        for url in tier {
            if let Ok(s) = std::str::from_utf8(url.as_ref())
                && !announce_urls.iter().any(|x| x == s)
            {
                announce_urls.push(s.to_string());
            }
        }
    }

    // librqbit 9.x wraps the info dict in `WithRawBytes` so the
    // computed raw bytes (for SHA-1) stay alongside the parsed view —
    // fields move from `info.*` to `info.data.*`. `iter_file_details`
    // also migrated to the post-`validate()` type and no longer returns
    // a `Result` (errors are checked at construction).
    let info_raw = &parsed.info.data;
    let name = match info_raw.name.as_ref() {
        Some(n) => {
            std::str::from_utf8(n.as_ref()).map_or_else(|_| "<invalid utf-8>".into(), str::to_owned)
        }
        None => "<unnamed>".into(),
    };
    let piece_length: u32 = info_raw.piece_length;
    let piece_count = u32::try_from(info_raw.pieces.as_ref().len() / 20).unwrap_or(u32::MAX);

    let info = info_raw.clone().validate()?;
    let mut files = Vec::new();
    let mut total: u64 = 0;
    for (idx, fd) in info.iter_file_details().enumerate() {
        let size: u64 = fd.len;
        total += size;
        let path_buf = fd.filename.to_pathbuf();
        let path = path_buf.to_string_lossy().to_string();
        let extension = path_buf
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_lowercase);
        let is_video = extension
            .as_deref()
            .is_some_and(|e| VIDEO_EXTS.contains(&e));
        files.push(TorrentFilePreview {
            index: idx,
            path,
            size_bytes: size,
            extension,
            is_video,
        });
    }

    Ok(TorrentPreview {
        infohash: info_hash,
        name,
        total_size_bytes: total,
        piece_length,
        piece_count,
        announce_urls,
        files,
    })
}
