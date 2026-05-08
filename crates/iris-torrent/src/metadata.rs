//! Pure metadata parsing — no network activity.

use librqbit::{ByteBuf, TorrentMetaV1, torrent_from_bytes_ext};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TorrentPreview {
    pub infohash: String,
    pub name: String,
    pub total_size_bytes: u64,
    pub piece_length: u32,
    pub piece_count: u32,
    pub announce_urls: Vec<String>,
    pub files: Vec<TorrentFilePreview>,
}

#[derive(Debug, Clone, Serialize)]
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
    let parsed: TorrentMetaV1<ByteBuf> = torrent_from_bytes_ext::<ByteBuf>(bytes)?.meta;
    let info_hash = hex::encode(parsed.info_hash.0);

    let mut announce_urls = Vec::new();
    if let Some(a) = parsed.announce.as_ref() {
        if let Ok(s) = std::str::from_utf8(a.as_ref()) {
            announce_urls.push(s.to_string());
        }
    }
    for tier in parsed.announce_list.iter() {
        for url in tier {
            if let Ok(s) = std::str::from_utf8(url.as_ref()) {
                if !announce_urls.iter().any(|x| x == s) {
                    announce_urls.push(s.to_string());
                }
            }
        }
    }

    let info = &parsed.info;
    let name = match info.name.as_ref() {
        Some(n) => std::str::from_utf8(n.as_ref())
            .map(str::to_owned)
            .unwrap_or_else(|_| "<invalid utf-8>".into()),
        None => "<unnamed>".into(),
    };
    let piece_length: u32 = info.piece_length;
    let piece_count = (info.pieces.as_ref().len() / 20) as u32;

    // iter_filenames_and_lengths gives an iterator independent of single/multi-file mode.
    let mut files = Vec::new();
    let mut total: u64 = 0;
    for (idx, fd) in info.iter_file_details()?.enumerate() {
        let size: u64 = fd.len;
        total += size;
        let path_buf = fd.filename.to_pathbuf()?;
        let path = path_buf.to_string_lossy().to_string();
        let extension = path_buf
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase());
        let is_video = extension
            .as_deref()
            .map(|e| VIDEO_EXTS.contains(&e))
            .unwrap_or(false);
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
