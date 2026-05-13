//! Extract subtitle streams as ``WebVTT``, ASS/SSA, or PGS bitmap (SUP).
//!
//! Three formats:
//! - **`WebVTT`**: ffmpeg transcodes text-based source codecs (`subrip`, `ass`,
//!   `mov_text`…) to `WebVTT`. The native `<track>` element handles it.
//! - **ASS/SSA**: text-based source remuxed as ASS without conversion.
//!   The client overlays it with `libass-wasm` to preserve positioning,
//!   styling, karaoke effects — features `WebVTT` can't represent.
//! - **PGS/SUP**: Blu-ray bitmap subtitles, copied verbatim. The client
//!   overlays them with `libpgs-js`. Cannot be losslessly transcoded.
//!
//! All formats stream the bytes as ffmpeg writes them (the browser starts
//! receiving immediately, important for slow MKV demuxes) and tee them
//! into a cache file so subsequent requests are static-file fast.

use std::fmt;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;

use bytes::Bytes;
use futures::Stream;
use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Debug, Error)]
pub enum SubtitleError {
    #[error("ffmpeg spawn failed: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("ffmpeg failed (status {0}): {1}")]
    Failed(i32, String),
}

/// Wire format the client requests via the `track.{ext}` URL suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleFormat {
    /// `track.vtt` — text codecs transcoded to `WebVTT`.
    WebVtt,
    /// `track.ass` — ASS/SSA copied (or normalised from SRT). The client
    /// renders via `libass-wasm`.
    Ass,
    /// `track.sup` — PGS bitmap copied verbatim. The client renders via
    /// `libpgs-js`.
    Sup,
}

impl SubtitleFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::WebVtt => "vtt",
            Self::Ass => "ass",
            Self::Sup => "sup",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Self::WebVtt => "text/vtt; charset=utf-8",
            Self::Ass => "text/x-ass; charset=utf-8",
            // No registered IANA MIME for PGS; libpgs-js reads bytes
            // raw, the value here is purely informational.
            Self::Sup => "application/octet-stream",
        }
    }

    /// ffmpeg `-c:s` value used when extracting this format.
    fn ffmpeg_codec(self) -> &'static str {
        match self {
            Self::WebVtt => "webvtt",
            Self::Ass => "ass",
            // PGS can't be transcoded by ffmpeg from another format and
            // is always emitted verbatim from the source.
            Self::Sup => "copy",
        }
    }

    /// ffmpeg `-f` (muxer) value used when extracting this format.
    fn ffmpeg_muxer(self) -> &'static str {
        match self {
            Self::WebVtt => "webvtt",
            Self::Ass => "ass",
            Self::Sup => "sup",
        }
    }
}

impl fmt::Display for SubtitleFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.extension())
    }
}

pub async fn extract_webvtt(
    source: &Path,
    idx_in_subtitles: u32,
) -> Result<String, SubtitleError> {
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error"])
        .arg("-i")
        .arg(source)
        .args(["-map", &format!("0:s:{idx_in_subtitles}")])
        .args(["-c:s", "webvtt", "-f", "webvtt", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        return Err(SubtitleError::Failed(
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// A boxed Stream the HTTP layer can hand to `axum::body::Body::from_stream`.
pub type SubtitleStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

/// Backwards-compat shim kept for the existing `/track.vtt` route.
pub async fn stream_webvtt(
    source: &Path,
    idx_in_subtitles: u32,
    cache_path: PathBuf,
) -> Result<SubtitleStream, SubtitleError> {
    stream_subtitle(source, idx_in_subtitles, SubtitleFormat::WebVtt, cache_path).await
}

/// Spawn `ffmpeg` to extract a subtitle stream in the requested format
/// and return a stream that emits chunks as ffmpeg produces them. Bytes
/// are also tee'd into `cache_path` so subsequent calls can short-
/// circuit. On clean exit the `.tmp` cache is renamed atomically; on
/// failure or early client disconnect the partial file is removed.
pub async fn stream_subtitle(
    source: &Path,
    idx_in_subtitles: u32,
    format: SubtitleFormat,
    cache_path: PathBuf,
) -> Result<SubtitleStream, SubtitleError> {
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let tmp_path = cache_path.with_extension(format!("{}.tmp", format.extension()));
    let _ = std::fs::remove_file(&tmp_path);

    let mut child = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "warning"])
        .arg("-i")
        .arg(source)
        .args(["-map", &format!("0:s:{idx_in_subtitles}")])
        .args(["-c:s", format.ffmpeg_codec(), "-f", format.ffmpeg_muxer(), "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("ffmpeg stdout missing"))?;

    if let Some(mut stderr) = child.stderr.take() {
        let target = format.extension();
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut reader = tokio::io::BufReader::new(&mut stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                tracing::debug!(target: "ffmpeg-sub", format = target, "{line}");
            }
        });
    }

    let cache_file = tokio::fs::File::create(&tmp_path).await?;
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(32);

    tokio::spawn(async move {
        let mut cache = cache_file;
        let mut buf = vec![0u8; 8 * 1024];
        let mut had_io_error = false;
        loop {
            match stdout.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if let Err(e) = cache.write_all(&buf[..n]).await {
                        tracing::warn!(error = %e, "subtitle cache write failed");
                        had_io_error = true;
                    }
                    let chunk = Bytes::copy_from_slice(&buf[..n]);
                    if tx.send(Ok(chunk)).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    had_io_error = true;
                    break;
                }
            }
        }
        let _ = cache.shutdown().await;
        drop(cache);
        match child.wait().await {
            Ok(status) if status.success() && !had_io_error => {
                if let Err(e) = tokio::fs::rename(&tmp_path, &cache_path).await {
                    tracing::warn!(error = %e, "subtitle cache promote failed");
                }
            }
            Ok(status) => {
                tracing::warn!(?status, "ffmpeg subtitle extraction exited non-zero");
                let _ = tokio::fs::remove_file(&tmp_path).await;
            }
            Err(e) => {
                tracing::warn!(error = %e, "ffmpeg subtitle extraction wait failed");
                let _ = tokio::fs::remove_file(&tmp_path).await;
            }
        }
    });

    Ok(Box::pin(ReceiverStream::new(rx)))
}

/// Computed cache path for a given (`infohash`, `file_idx`, `sub_idx`,
/// format). Files of different formats coexist (e.g., a track can be
/// served as both `WebVTT` and ASS).
pub fn cache_path(
    base_dir: &Path,
    infohash: &str,
    file_idx: usize,
    sub_idx: u32,
    format: SubtitleFormat,
) -> PathBuf {
    base_dir.join(format!(
        "{infohash}_{file_idx}_{sub_idx}.{}",
        format.extension()
    ))
}
