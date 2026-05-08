//! Persistent ffmpeg HLS muxer + synthetic master playlist.
//!
//! For each `(infohash, file_idx, audio_idx)` key we spawn a single ffmpeg
//! process that writes properly-aligned MPEG-TS segments to disk via the
//! HLS muxer. The HTTP layer answers `master.m3u8` instantly with a
//! synthetic VOD playlist derived from probe duration; segment requests
//! poll the disk and return as soon as ffmpeg writes the corresponding
//! `segment_NNNNN.ts`.
//!
//! On-demand single-segment generation was tried and **abandoned**: with
//! `-c:v copy` each independent ffmpeg call has slightly off PTS (because
//! `-ss BEFORE -i` snaps to the previous keyframe), making hls.js see the
//! timeline rewind on every seek. Letting ffmpeg muxer produce the whole
//! sequence keeps timestamps monotonic across all segments.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use thiserror::Error;
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, Notify};
use tokio::time::timeout;

#[derive(Debug, Error)]
pub enum HlsError {
    #[error("ffmpeg spawn failed: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("ffmpeg failed (status {0}): {1}")]
    Failed(i32, String),
    #[error("timeout waiting for {1}")]
    Timeout(Duration, String),
}

#[derive(Clone)]
pub struct HlsManager {
    inner: Arc<Inner>,
}

struct Inner {
    base_dir: PathBuf,
    jobs: Arc<Mutex<HashMap<String, Arc<JobState>>>>,
}

struct JobState {
    dir: PathBuf,
    /// Pinged when ffmpeg exits (success or failure) so segment waiters
    /// don't poll forever for a file that will never appear.
    finished: Notify,
    /// Set true if ffmpeg exited non-zero / failed to start.
    failed: AtomicBool,
}

impl HlsManager {
    pub fn new(base_dir: PathBuf) -> Self {
        Self {
            inner: Arc::new(Inner {
                base_dir,
                jobs: Arc::new(Mutex::new(HashMap::new())),
            }),
        }
    }

    pub fn segment_dir_for(&self, key: &str) -> PathBuf {
        self.inner.base_dir.join(key)
    }

    pub fn segment_path(&self, key: &str, segment_idx: u32) -> PathBuf {
        self.segment_dir_for(key)
            .join(format!("segment_{segment_idx:05}.ts"))
    }

    /// Read the master playlist that ffmpeg has written so far. This is the
    /// authoritative source of truth for segment durations — synthesizing
    /// our own with arbitrary 6-second EXTINF entries breaks playback when
    /// the source has long GOPs (ffmpeg snaps segments to keyframes, so a
    /// file with 12s GOPs ends up with 12s segments and our 6s claim makes
    /// hls.js compute everything wrong).
    ///
    /// Waits up to `max_wait` for the playlist to either complete
    /// (`#EXT-X-ENDLIST` present) or carry at least `min_segments` so the
    /// player can start playing. Returns whatever was on disk after the wait.
    pub async fn read_master_playlist(
        &self,
        key: &str,
        min_segments: usize,
        max_wait: Duration,
    ) -> Result<String, HlsError> {
        let dir = self.segment_dir_for(key);
        let path = dir.join("ffmpeg_master.m3u8");
        let job = {
            let jobs = self.inner.jobs.lock().await;
            jobs.get(key).cloned()
        };
        let target = path.clone();
        let display = target.display().to_string();
        let res = timeout(max_wait, async {
            let poll = async {
                loop {
                    if let Ok(content) = tokio::fs::read_to_string(&target).await {
                        let count = content.matches("#EXTINF").count();
                        let done = content.contains("#EXT-X-ENDLIST");
                        if done || count >= min_segments {
                            return Ok::<String, ()>(content);
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(150)).await;
                }
            };
            if let Some(job) = job {
                tokio::select! {
                    r = poll => r,
                    _ = job.finished.notified() => {
                        tokio::fs::read_to_string(&target).await.map_err(|_| ())
                    }
                }
            } else {
                poll.await
            }
        })
        .await;
        match res {
            Ok(Ok(s)) => Ok(s),
            _ => {
                // Last-ditch: return whatever is on disk.
                tokio::fs::read_to_string(&path).await.map_err(|_| {
                    HlsError::Timeout(max_wait, display.clone())
                })
            }
        }
    }

    /// Ensure an ffmpeg HLS job is running for `key`; returns the directory
    /// once spawned (or already running). Segments arrive on disk as ffmpeg
    /// processes the file.
    pub async fn ensure_job(
        &self,
        key: &str,
        source: &Path,
        audio_track: u32,
        audio_codec_in_source: Option<&str>,
    ) -> Result<PathBuf, HlsError> {
        let dir = self.segment_dir_for(key);

        let mut jobs = self.inner.jobs.lock().await;
        if let Some(j) = jobs.get(key) {
            return Ok(j.dir.clone());
        }
        // If a previous job already finished and segments are on disk, no
        // need to respawn — they'll just be served as static files. We keep
        // the dedupe entry empty in that case; reaper removed it on exit.
        if dir.join("segment_00000.ts").exists() {
            return Ok(dir);
        }

        let j = Arc::new(JobState {
            dir: dir.clone(),
            finished: Notify::new(),
            failed: AtomicBool::new(false),
        });
        jobs.insert(key.to_string(), j.clone());
        drop(jobs);

        std::fs::create_dir_all(&dir)?;
        let child = spawn_ffmpeg(source, &dir, audio_track, audio_codec_in_source)?;

        let key_owned = key.to_string();
        let jobs_handle = self.inner.jobs.clone();
        let job_for_reaper = j.clone();
        tokio::spawn(async move {
            let exit = await_child_exit(child).await;
            match exit {
                Ok(status) if status.success() => {
                    tracing::info!(key = %key_owned, "ffmpeg HLS job finished");
                }
                Ok(status) => {
                    tracing::warn!(key = %key_owned, ?status, "ffmpeg HLS exited non-zero");
                    job_for_reaper.failed.store(true, Ordering::Release);
                }
                Err(e) => {
                    tracing::warn!(key = %key_owned, error = %e, "ffmpeg HLS wait failed");
                    job_for_reaper.failed.store(true, Ordering::Release);
                }
            }
            job_for_reaper.finished.notify_waiters();
            let mut jobs = jobs_handle.lock().await;
            jobs.remove(&key_owned);
        });

        Ok(dir)
    }

    /// Wait until a specific segment file exists on disk, or ffmpeg exits,
    /// or `max` elapses.
    pub async fn wait_for_segment(
        &self,
        key: &str,
        path: &Path,
        max: Duration,
    ) -> Result<(), HlsError> {
        if path.exists() {
            return Ok(());
        }
        let job = {
            let jobs = self.inner.jobs.lock().await;
            jobs.get(key).cloned()
        };
        let target = path.to_path_buf();
        let display = target.display().to_string();
        let res = timeout(max, async {
            let poll = async {
                loop {
                    if target.exists() {
                        return Ok::<(), ()>(());
                    }
                    tokio::time::sleep(Duration::from_millis(150)).await;
                }
            };
            if let Some(job) = job {
                tokio::select! {
                    r = poll => r,
                    _ = job.finished.notified() => {
                        if target.exists() { Ok(()) } else { Err(()) }
                    }
                }
            } else {
                poll.await
            }
        })
        .await;
        match res {
            Ok(Ok(())) => Ok(()),
            Ok(Err(())) => Err(HlsError::Timeout(
                Duration::from_secs(0),
                format!("ffmpeg exited before {display} was ready"),
            )),
            Err(_) => Err(HlsError::Timeout(max, display)),
        }
    }

    pub async fn cleanup_for_torrent(&self, infohash: &str) {
        let prefix = format!("{infohash}_");
        let mut to_remove = Vec::new();
        if let Ok(read) = std::fs::read_dir(&self.inner.base_dir) {
            for entry in read.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.starts_with(&prefix) || name == infohash {
                        to_remove.push(entry.path());
                    }
                }
            }
        }
        for p in to_remove {
            if let Err(e) = tokio::fs::remove_dir_all(&p).await {
                tracing::warn!(path = %p.display(), error = %e, "hls cleanup failed");
            }
        }
        let mut jobs = self.inner.jobs.lock().await;
        jobs.retain(|k, _| !k.starts_with(infohash));
    }
}

fn spawn_ffmpeg(
    source: &Path,
    dir: &Path,
    audio_track: u32,
    audio_codec_in_source: Option<&str>,
) -> Result<Child, HlsError> {
    let master = dir.join("ffmpeg_master.m3u8"); // not served — we synthesize ours
    let segment_pattern = dir.join("segment_%05d.ts");

    let copy_audio = matches!(
        audio_codec_in_source.map(|s| s.to_ascii_lowercase()),
        Some(c) if c == "aac" || c == "mp3"
    );

    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-hide_banner", "-loglevel", "warning", "-y"])
        .arg("-i")
        .arg(source)
        .args(["-map", "0:v:0", "-c:v", "copy"])
        .args(["-map", &format!("0:a:{audio_track}")]);
    if copy_audio {
        cmd.args(["-c:a", "copy"]);
    } else {
        cmd.args(["-c:a", "aac", "-ac", "2", "-b:a", "192k"]);
    }
    cmd.args(["-sn"])
        .args(["-hls_time", "6"])
        .args(["-hls_list_size", "0"])
        .args(["-hls_segment_type", "mpegts"])
        .args(["-hls_flags", "temp_file+independent_segments"])
        .arg("-hls_segment_filename")
        .arg(&segment_pattern)
        .args(["-hls_playlist_type", "vod"])
        .args(["-f", "hls"])
        .arg(&master)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    tracing::info!(
        source = %source.display(),
        dir = %dir.display(),
        audio_track,
        copy_audio,
        "spawning persistent ffmpeg HLS job"
    );

    let mut child = cmd.spawn()?;
    if let Some(mut stderr) = child.stderr.take() {
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut reader = tokio::io::BufReader::new(&mut stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                tracing::debug!(target: "ffmpeg-hls", "{line}");
            }
        });
    }
    Ok(child)
}

async fn await_child_exit(mut child: Child) -> std::io::Result<std::process::ExitStatus> {
    child.wait().await
}
