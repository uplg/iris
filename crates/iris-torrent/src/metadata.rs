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
    /// `false` when the release is archive-dominant (scene RAR sets:
    /// `.rar`/`.rXX`/split volumes outweigh the video bytes — any video
    /// present is just a sample) or carries no video file at all. Iris
    /// streams straight from the video container, so such a release can
    /// be seeded but never played.
    pub streamable: bool,
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
    /// Heuristic: `true` for packed-archive volumes (`.rar`, `.zip`,
    /// `.7z`, `.rXX` / `.NNN` split volumes).
    pub is_archive: bool,
}

const VIDEO_EXTS: &[&str] = &[
    "mkv", "mp4", "m4v", "avi", "mov", "webm", "ts", "mts", "m2ts", "wmv",
];

/// `.rar`/`.zip`/`.7z` plus the classic split-volume tails: `.r00`…`.r99`
/// and the hjsplit-style all-digit `.001`…`.999`.
fn is_archive_ext(ext: &str) -> bool {
    matches!(ext, "rar" | "zip" | "7z")
        || (ext.len() == 3 && ext.starts_with('r') && ext[1..].bytes().all(|b| b.is_ascii_digit()))
        || (ext.len() == 3 && ext.bytes().all(|b| b.is_ascii_digit()))
}

/// Byte-weighted, not file-counted: a RAR'd movie still ships a tiny
/// `sample.mkv` (video present, archives dominant → not streamable),
/// while a season pack ships 20 mkvs and zero archives.
fn compute_streamable(files: &[TorrentFilePreview]) -> bool {
    let video_bytes: u64 = files
        .iter()
        .filter(|f| f.is_video)
        .map(|f| f.size_bytes)
        .sum();
    let archive_bytes: u64 = files
        .iter()
        .filter(|f| f.is_archive)
        .map(|f| f.size_bytes)
        .sum();
    video_bytes > 0 && video_bytes >= archive_bytes
}

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
        let is_archive = extension.as_deref().is_some_and(is_archive_ext);
        files.push(TorrentFilePreview {
            index: idx,
            path,
            size_bytes: size,
            extension,
            is_video,
            is_archive,
        });
    }

    let streamable = compute_streamable(&files);

    Ok(TorrentPreview {
        infohash: info_hash,
        name,
        total_size_bytes: total,
        piece_length,
        piece_count,
        announce_urls,
        files,
        streamable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, size: u64) -> TorrentFilePreview {
        let extension = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_lowercase);
        let is_video = extension
            .as_deref()
            .is_some_and(|e| VIDEO_EXTS.contains(&e));
        let is_archive = extension.as_deref().is_some_and(is_archive_ext);
        TorrentFilePreview {
            index: 0,
            path: path.into(),
            size_bytes: size,
            extension,
            is_video,
            is_archive,
        }
    }

    #[test]
    fn archive_extensions() {
        for ext in ["rar", "r00", "r37", "zip", "7z", "001", "999"] {
            assert!(is_archive_ext(ext), "{ext} should classify as archive");
        }
        for ext in ["mkv", "nfo", "srt", "r1", "rev", "part1", "mp4"] {
            assert!(!is_archive_ext(ext), "{ext} should NOT classify as archive");
        }
    }

    #[test]
    fn rared_movie_with_sample_is_not_streamable() {
        let mut files = vec![file("Movie.2024.1080p-GRP/movie-grp.rar", 100_000_000)];
        for i in 0..80 {
            files.push(file(
                &format!("Movie.2024.1080p-GRP/movie-grp.r{i:02}"),
                100_000_000,
            ));
        }
        files.push(file(
            "Movie.2024.1080p-GRP/Sample/movie-sample.mkv",
            60_000_000,
        ));
        files.push(file("Movie.2024.1080p-GRP/movie.nfo", 20_000));
        assert!(!compute_streamable(&files));
    }

    #[test]
    fn plain_mkv_movie_is_streamable() {
        let files = vec![
            file("Movie.2024.1080p-GRP.mkv", 8_000_000_000),
            file("Movie.2024.1080p-GRP.nfo", 20_000),
        ];
        assert!(compute_streamable(&files));
    }

    #[test]
    fn season_pack_of_mkvs_is_streamable() {
        let files: Vec<_> = (1..=20)
            .map(|e| file(&format!("Show.S01/Show.S01E{e:02}.mkv"), 1_500_000_000))
            .collect();
        assert!(compute_streamable(&files));
    }

    #[test]
    fn archive_only_release_is_not_streamable() {
        let files = vec![
            file("App.Name-GRP/app-grp.rar", 50_000_000),
            file("App.Name-GRP/app-grp.r00", 50_000_000),
        ];
        assert!(!compute_streamable(&files));
    }

    #[test]
    fn movie_with_incidental_zip_extras_is_streamable() {
        let files = vec![
            file("Movie.2024.mkv", 8_000_000_000),
            file("Extras/artwork.zip", 30_000_000),
        ];
        assert!(compute_streamable(&files));
    }
}
