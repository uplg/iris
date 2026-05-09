//! Convert text-based subtitle streams to `WebVTT`.
//!
//! Two flavors:
//! - [`extract_webvtt`]: blocking (collects all output to memory). Used for
//!   tiny streams or one-shot scripts.
//! - [`stream_webvtt`]: spawns ffmpeg, returns a `Stream<Item = Bytes>` that
//!   yields chunks as ffmpeg writes them, *and* tees them into a cache file.
//!   This is what the HTTP layer uses so the browser starts receiving bytes
//!   immediately (otherwise Firefox aborts `<track>` requests with
//!   `NS_BINDING_ERROR` while ffmpeg scans large MKVs).

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

/// Spawn `ffmpeg` to extract a subtitle stream as `WebVTT` and return a stream
/// that emits chunks as ffmpeg produces them. Bytes are also tee'd into
/// `cache_path` so subsequent calls can short-circuit. On clean exit the
/// `.tmp` cache is renamed atomically; on failure or early client
/// disconnect the partial file is removed.
pub async fn stream_webvtt(
    source: &Path,
    idx_in_subtitles: u32,
    cache_path: PathBuf,
) -> Result<SubtitleStream, SubtitleError> {
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let tmp_path = cache_path.with_extension("vtt.tmp");
    // Best-effort cleanup of any leftover partial file from a previous run.
    let _ = std::fs::remove_file(&tmp_path);

    let mut child = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "warning"])
        .arg("-i")
        .arg(source)
        .args(["-map", &format!("0:s:{idx_in_subtitles}")])
        .args(["-c:s", "webvtt", "-f", "webvtt", "pipe:1"])
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
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut reader = tokio::io::BufReader::new(&mut stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                tracing::debug!(target: "ffmpeg-vtt", "{line}");
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
                        // Client disconnected. Stop tee'ing; ffmpeg will be
                        // killed when `child` is dropped at the end of this
                        // task because of `kill_on_drop`.
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

/// Computed cache path for a given (`infohash`, `file_idx`, `sub_idx`).
pub fn cache_path(base_dir: &Path, infohash: &str, file_idx: usize, sub_idx: u32) -> PathBuf {
    base_dir.join(format!("{infohash}_{file_idx}_{sub_idx}.vtt"))
}
