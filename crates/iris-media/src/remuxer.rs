//! HLS-CMAF remuxer.
//!
//! For each source file we run *one* `ffmpeg -c copy` invocation that
//! produces:
//!   * a master `.m3u8` playlist with the video stream + every audio
//!     rendition declared as `EXT-X-MEDIA` in a single audio group,
//!   * one variant playlist per stream (`v.m3u8`, `<name>.m3u8`), each
//!     pointing into a single CMAF fragmented MP4 file via
//!     `EXT-X-BYTERANGE`,
//!   * one `<name>.m4s` per stream containing the init segment + every
//!     fragment back-to-back (`-hls_flags +single_file`).
//!
//! Why HLS-CMAF over a single multi-track fMP4:
//!   * Browsers and Android Media3 only expose multi-audio reliably via
//!     a manifest-driven engine (`hls.js`, `ExoPlayer`'s HLS source). A
//!     single multi-track MP4 typically only surfaces the first audio.
//!   * The manifest tells the player exact byte offsets per fragment, so
//!     Chrome stops fan-out scanning the file looking for moofs (the
//!     "flood" we saw on a sidx-less progressive fMP4).
//!   * Playlist type `EVENT` lets the player follow the encode while
//!     ffmpeg is still running, no two-pass dance, no patched moov.
//!
//! Failure modes are unforgiving on purpose: when ffmpeg exits non-zero
//! we wipe the half-baked output directory so the next call retries
//! from scratch. Sticky-failure cache prevents respawn storms on
//! deterministic errors.
//!
//! Subtitles stay external (served as `WebVTT` by a dedicated route).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use thiserror::Error;
use tokio::io::AsyncBufReadExt;
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, Notify};

#[derive(Debug, Error)]
pub enum RemuxError {
    #[error("ffmpeg spawn failed: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("ffmpeg failed (status {0}); see {1}")]
    Failed(i32, String),
    #[error("a concurrent remux for the same key failed")]
    SharedFailure,
}

/// Master playlist filename, served as the entry point.
pub const MASTER_PLAYLIST: &str = "master.m3u8";
/// Video variant name (used in URL + filenames inside the cache dir).
const VIDEO_VARIANT: &str = "v";

/// How long a recorded ffmpeg failure is treated as still-fresh. After this
/// the next caller is allowed to retry from scratch — covers transient flakes
/// without spamming during a hard failure.
const FAILURE_COOLDOWN: Duration = Duration::from_secs(5 * 60);

/// Per-remux configuration derived from probe data by the caller.
///
/// Each `audio` entry becomes one HLS audio rendition in the master
/// playlist's audio group. We never include the same source audio twice
/// (no parallel "copy + AAC fallback" — for HLS the rendition list IS
/// the user-facing menu, so duplicating tracks confuses everyone).
/// Browsers can decode only AAC / MP3 / Opus / Vorbis natively, so any
/// audio in another codec MUST be transcoded to AAC.
#[derive(Debug, Clone)]
pub struct RemuxPlan {
    pub audio: Vec<AudioRendition>,
    /// Source video codec name (e.g. `"hevc"`, `"h264"`). Used to decide
    /// whether to force `-tag:v hvc1` (HEVC only).
    pub source_video_codec: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AudioRendition {
    /// `0:a:N` source index.
    pub source_idx: usize,
    /// `Copy` is only safe for codecs the browser decodes natively
    /// (AAC / MP3 / Opus / Vorbis). Otherwise pick `Aac` to transcode.
    pub codec: AudioCodec,
    /// Short id used as the variant filename (`<name>.m3u8` /
    /// `<name>.m4s`) AND as the rendition `NAME` in the master playlist.
    /// MUST be unique within a plan and contain only `[a-z0-9_-]`.
    pub name: String,
    /// ISO 639-2 language code, e.g. `"fre"`, `"eng"`. `"und"` if unknown.
    pub language: String,
    /// Set on exactly one rendition. Becomes `DEFAULT=YES` in the manifest.
    pub default: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum AudioCodec {
    Copy,
    Aac,
}

impl RemuxPlan {
    pub fn copy_only_with_languages(audio_languages: &[Option<String>]) -> Self {
        Self {
            audio: audio_languages
                .iter()
                .enumerate()
                .map(|(i, lang)| AudioRendition {
                    source_idx: i,
                    codec: AudioCodec::Copy,
                    name: format!("audio_{i}"),
                    language: lang.clone().unwrap_or_else(|| "und".into()),
                    default: i == 0,
                })
                .collect(),
            source_video_codec: None,
        }
    }
}

#[derive(Clone)]
pub struct RemuxManager {
    inner: Arc<Inner>,
}

struct Inner {
    base_dir: PathBuf,
    /// In-flight remux jobs keyed by `<infohash>_<file_idx>`. Multiple
    /// concurrent `ensure_remuxed` calls for the same key all attach to
    /// the same `JobState` and wake together.
    jobs: Arc<Mutex<std::collections::HashMap<String, Arc<JobState>>>>,
    /// Sticky failure cache. ffmpeg errors are deterministic on the same
    /// input — without this, every poll on `play_status` would respawn
    /// the same losing command, hammering the server.
    failures: Arc<Mutex<std::collections::HashMap<String, Failure>>>,
}

#[derive(Clone)]
struct Failure {
    at: Instant,
    message: String,
}

struct JobState {
    /// Wakes when the master playlist + at least the first fragment of
    /// each variant are on disk — i.e. the player can start consuming
    /// the manifest. Also fires when ffmpeg exits early so failed jobs
    /// unblock waiters.
    ready: Notify,
    /// True once `ready` has been (or is about to be) signalled. Waiters
    /// check this BEFORE awaiting the notify, otherwise they'd hang if
    /// the watcher fired before they registered.
    signaled: AtomicBool,
    /// Set by the watcher / reaper when ffmpeg exits non-zero or never
    /// produced a usable manifest.
    failed: AtomicBool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct JobInfo {
    pub key: String,
    pub size_bytes: u64,
    pub mtime: Option<i64>,
    pub in_flight: bool,
}

impl RemuxManager {
    pub fn new(base_dir: PathBuf) -> Self {
        Self {
            inner: Arc::new(Inner {
                base_dir,
                jobs: Arc::new(Mutex::new(std::collections::HashMap::new())),
                failures: Arc::new(Mutex::new(std::collections::HashMap::new())),
            }),
        }
    }

    pub fn base_dir(&self) -> &Path {
        &self.inner.base_dir
    }

    /// Directory holding the master playlist, variant playlists and
    /// `.m4s` files for `key`. Returned regardless of whether ffmpeg has
    /// produced anything yet.
    pub fn cache_dir(&self, key: &str) -> PathBuf {
        self.inner.base_dir.join(key)
    }

    /// Path of the master playlist served as the player entry point.
    pub fn master_path(&self, key: &str) -> PathBuf {
        self.cache_dir(key).join(MASTER_PLAYLIST)
    }

    /// Resolve `<cache_dir>/<asset>` while rejecting traversal. Returns
    /// `None` when the asset name is unsafe.
    pub fn asset_path(&self, key: &str, asset: &str) -> Option<PathBuf> {
        if !is_safe_asset_name(asset) {
            return None;
        }
        Some(self.cache_dir(key).join(asset))
    }

    /// Ffmpeg log path (sidecar at the cache dir level).
    fn log_path(&self, key: &str) -> PathBuf {
        self.cache_dir(key).join("ffmpeg.log")
    }

    /// Returns the recorded error message for `key` if a recent ffmpeg
    /// run failed and the cooldown hasn't elapsed.
    pub async fn recent_failure(&self, key: &str) -> Option<String> {
        let mut failures = self.inner.failures.lock().await;
        if let Some(f) = failures.get(key) {
            if f.at.elapsed() < FAILURE_COOLDOWN {
                return Some(f.message.clone());
            }
            failures.remove(key);
        }
        None
    }

    /// Block until the master playlist + first fragment of each variant
    /// are on disk. Spawns ffmpeg if no cache exists and no other call
    /// has it in flight; otherwise piggybacks on the active job.
    pub async fn ensure_remuxed(
        &self,
        key: &str,
        source: &Path,
        plan: RemuxPlan,
    ) -> Result<PathBuf, RemuxError> {
        let master = self.master_path(key);

        // Lock-free fast path: cache hit.
        if master_is_ready(&master).await {
            return Ok(master);
        }

        if let Some(msg) = self.recent_failure(key).await {
            return Err(RemuxError::Failed(0, msg));
        }

        let job = {
            let mut jobs = self.inner.jobs.lock().await;
            if let Some(existing) = jobs.get(key).cloned() {
                existing
            } else {
                if master_is_ready(&master).await {
                    return Ok(master);
                }
                let job = Arc::new(JobState {
                    ready: Notify::new(),
                    signaled: AtomicBool::new(false),
                    failed: AtomicBool::new(false),
                });
                jobs.insert(key.to_string(), job.clone());
                self.spawn_remux(key.to_string(), source.to_path_buf(), plan, job.clone());
                job
            }
        };

        let waiter = job.ready.notified();
        tokio::pin!(waiter);
        if !job.signaled.load(Ordering::Acquire) {
            waiter.await;
        }
        if job.failed.load(Ordering::Acquire) {
            return Err(RemuxError::SharedFailure);
        }
        if !master_is_ready(&master).await {
            return Err(RemuxError::Failed(0, master.display().to_string()));
        }
        Ok(master)
    }

    /// Drop the cache for one key, returning the freed bytes.
    pub async fn wipe(&self, key: &str) -> Result<u64, RemuxError> {
        let dir = self.cache_dir(key);
        let freed = dir_size(&dir).await.unwrap_or(0);
        let _ = tokio::fs::remove_dir_all(&dir).await;
        // Wiping is the user's escape hatch from a sticky failure: clear
        // it so the next play attempt actually re-runs ffmpeg.
        self.inner.failures.lock().await.remove(key);
        Ok(freed)
    }

    /// Evict cache entries (oldest mtime first) until total size ≤ cap.
    /// In-flight jobs are skipped. Returns the number of dirs removed.
    pub async fn evict_to(&self, max_total_bytes: u64) -> usize {
        let active: HashSet<String> = self.inner.jobs.lock().await.keys().cloned().collect();
        let mut entries: Vec<(PathBuf, u64, SystemTime, String)> = Vec::new();
        let Ok(mut rd) = tokio::fs::read_dir(&self.inner.base_dir).await else {
            return 0;
        };
        while let Ok(Some(entry)) = rd.next_entry().await {
            let Ok(meta) = entry.metadata().await else {
                continue;
            };
            if !meta.is_dir() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if active.contains(&name) {
                continue;
            }
            let size = dir_size(&entry.path()).await.unwrap_or(0);
            let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            entries.push((entry.path(), size, mtime, name));
        }
        let total: u64 = entries.iter().map(|(_, sz, _, _)| sz).sum();
        if total <= max_total_bytes {
            return 0;
        }
        entries.sort_by_key(|(_, _, mtime, _)| *mtime);
        let mut current = total;
        let mut evicted = 0usize;
        for (path, sz, _, key) in entries {
            if current <= max_total_bytes {
                break;
            }
            if let Err(e) = tokio::fs::remove_dir_all(&path).await {
                tracing::warn!(path = %path.display(), error = %e, "remuxer: evict failed");
                continue;
            }
            current -= sz;
            evicted += 1;
            tracing::info!(key = %key, freed_bytes = sz, "remuxer: evicted cache");
        }
        evicted
    }

    /// Inventory of cache dirs for the admin endpoint.
    pub async fn list_jobs(&self) -> Vec<JobInfo> {
        let active: HashSet<String> = self.inner.jobs.lock().await.keys().cloned().collect();
        let mut out: Vec<JobInfo> = Vec::new();
        let Ok(mut rd) = tokio::fs::read_dir(&self.inner.base_dir).await else {
            return out;
        };
        while let Ok(Some(entry)) = rd.next_entry().await {
            let Ok(meta) = entry.metadata().await else {
                continue;
            };
            if !meta.is_dir() {
                continue;
            }
            let Some(key) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let size = dir_size(&entry.path()).await.unwrap_or(0);
            let mtime_secs = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .and_then(|d| i64::try_from(d.as_secs()).ok());
            let in_flight = active.contains(&key);
            out.push(JobInfo {
                key,
                size_bytes: size,
                mtime: mtime_secs,
                in_flight,
            });
        }
        for key in active {
            if !out.iter().any(|j| j.key == key) {
                out.push(JobInfo {
                    key,
                    size_bytes: 0,
                    mtime: None,
                    in_flight: true,
                });
            }
        }
        out.sort_by_key(|j| std::cmp::Reverse(j.mtime));
        out
    }

    fn spawn_remux(&self, key: String, source: PathBuf, plan: RemuxPlan, job: Arc<JobState>) {
        let dir = self.cache_dir(&key);
        let master = self.master_path(&key);
        let log_path = self.log_path(&key);
        let jobs_handle = self.inner.jobs.clone();
        let failures_handle = self.inner.failures.clone();

        // Watcher: polls the cache dir until the master playlist + at
        // least the first segment of every declared variant are on disk.
        // Once that holds we wake any caller blocked on `ensure_remuxed`,
        // even though ffmpeg keeps appending new segments.
        {
            let job = job.clone();
            let master = master.clone();
            let dir = dir.clone();
            let variants = plan.expected_variants();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    if job.signaled.load(Ordering::Acquire) {
                        return;
                    }
                    if hls_is_ready(&master, &dir, &variants).await {
                        job.signaled.store(true, Ordering::Release);
                        job.ready.notify_waiters();
                        return;
                    }
                }
            });
        }

        tokio::spawn(async move {
            let failure_message: Option<String>;
            if let Err(e) = tokio::fs::create_dir_all(&dir).await {
                tracing::warn!(error = %e, dir = %dir.display(), "remuxer: mkdir failed");
                job.failed.store(true, Ordering::Release);
                failure_message = Some(format!("mkdir {}: {e}", dir.display()));
            } else {
                match remux_one(&source, &dir, &log_path, &plan).await {
                    Ok(()) => {
                        tracing::info!(
                            key = %key,
                            dir = %dir.display(),
                            "remuxer: cache built",
                        );
                        failure_message = None;
                    }
                    Err(e) => {
                        // Read the ffmpeg log BEFORE wiping so the user
                        // (and the sticky-failure message in PlayStatus)
                        // gets the real reason ffmpeg bailed instead of
                        // just "status N".
                        let tail = read_log_tail(&log_path, 1024).await;
                        let combined = match tail {
                            Some(t) if !t.trim().is_empty() => format!("{e} :: {t}"),
                            _ => e.to_string(),
                        };
                        tracing::warn!(
                            key = %key,
                            error = %combined,
                            "remuxer: ffmpeg failed",
                        );
                        // Half-baked dir is worthless and would confuse a
                        // retry: nuke it. The failure_message captured
                        // above is what gets surfaced via PlayStatus.
                        let _ = tokio::fs::remove_dir_all(&dir).await;
                        job.failed.store(true, Ordering::Release);
                        failure_message = Some(combined);
                    }
                }
            }
            {
                let mut failures = failures_handle.lock().await;
                if let Some(msg) = failure_message {
                    failures.insert(key.clone(), Failure { at: Instant::now(), message: msg });
                } else {
                    failures.remove(&key);
                }
            }
            // Wake any callers still blocked on `ready` — covers the case
            // where ffmpeg failed (or finished too fast) before the
            // watcher noticed first-segment readiness.
            job.signaled.store(true, Ordering::Release);
            job.ready.notify_waiters();
            jobs_handle.lock().await.remove(&key);
        });
    }
}

impl RemuxPlan {
    /// Variant init segments the watcher waits on. shaka-packager
    /// writes `<name>_init.mp4` per variant + numbered `<name>_N.m4s`
    /// fragments; the existence of every init segment is sufficient
    /// proof that the pipeline has produced playable output for every
    /// rendition (the master playlist references the init segments and
    /// fragments by name).
    fn expected_variants(&self) -> Vec<String> {
        let mut v = vec![format!("{VIDEO_VARIANT}_init.mp4")];
        for a in &self.audio {
            v.push(format!("{}_init.mp4", a.name));
        }
        v
    }
}

async fn master_is_ready(master: &Path) -> bool {
    matches!(
        tokio::fs::metadata(master).await,
        Ok(m) if m.is_file() && m.len() > 0
    )
}

/// True iff the master playlist and the first segment of every declared
/// variant are on disk. We don't actually re-parse the manifests — the
/// presence of every `<name>.m4s` under the cache dir is enough proof
/// that ffmpeg has progressed past the initial pre-roll.
async fn hls_is_ready(master: &Path, dir: &Path, variants: &[String]) -> bool {
    if !master_is_ready(master).await {
        return false;
    }
    for v in variants {
        let p = dir.join(v);
        let ok = matches!(
            tokio::fs::metadata(&p).await,
            Ok(m) if m.is_file() && m.len() > 0
        );
        if !ok {
            return false;
        }
    }
    true
}

async fn dir_size(path: &Path) -> std::io::Result<u64> {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(p) = stack.pop() {
        let mut rd = match tokio::fs::read_dir(&p).await {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        while let Some(e) = rd.next_entry().await? {
            let Ok(m) = e.metadata().await else {
                continue;
            };
            if m.is_dir() {
                stack.push(e.path());
            } else {
                total += m.len();
            }
        }
    }
    Ok(total)
}

/// Asset names accepted by `/play/{*asset}`. Matches every file ffmpeg
/// produces under the cache dir, rejects everything else (path traversal
/// segments like `..`, absolute paths, control characters).
fn is_safe_asset_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() < 128
        && !name.contains("..")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// Two-stage HLS-CMAF pipeline: ffmpeg encodes per-stream MP4 outputs
/// to a temp dir, then shaka-packager ingests those and writes the
/// HLS-CMAF tree (master playlist + per-rendition playlists + init
/// segments + numbered `.m4s` segments) into the cache dir.
///
/// Why shaka-packager rather than ffmpeg's HLS muxer:
///   * shaka writes the CODECS attribute in a form browsers' MSE
///     actually accepts. ffmpeg emits hyper-strict strings like
///     `hvc1.2.4.L120.B01` that Chrome rejects up-front via
///     `MediaSource.isTypeSupported`, even on setups that can decode
///     the bytes — silent failure with no playback.
///   * Industry-standard manifest output (Netflix / `YouTube` use shaka).
///   * Proper init-segment hvcC / esds boxes, byte-accurate segment
///     templating, no half-baked atoms.
///
/// Trade-off: temp files double the peak disk usage during a remux
/// (source -> ffmpeg temps -> shaka final). They're wiped on success,
/// so steady-state cache cost is unchanged.
async fn remux_one(
    source: &Path,
    out_dir: &Path,
    log_path: &Path,
    plan: &RemuxPlan,
) -> Result<(), RemuxError> {
    let temp_dir = out_dir.join(".tmp");
    if tokio::fs::try_exists(&temp_dir).await.unwrap_or(false) {
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }
    tokio::fs::create_dir_all(&temp_dir).await?;

    // Stage 1: ffmpeg → per-stream MP4s in temp_dir.
    let video_tmp = temp_dir.join("video.mp4");
    let audio_tmps: Vec<PathBuf> = (0..plan.audio.len())
        .map(|i| temp_dir.join(format!("audio_{i}.mp4")))
        .collect();
    run_ffmpeg(source, &video_tmp, &audio_tmps, plan, log_path).await?;

    // Stage 2: shaka-packager → HLS-CMAF in out_dir.
    run_shaka(out_dir, &video_tmp, &audio_tmps, plan, log_path).await?;

    // Done — the temp inputs aren't needed any more.
    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    Ok(())
}

async fn run_ffmpeg(
    source: &Path,
    video_tmp: &Path,
    audio_tmps: &[PathBuf],
    plan: &RemuxPlan,
    log_path: &Path,
) -> Result<(), RemuxError> {
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-hide_banner", "-loglevel", "error", "-y"])
        .arg("-i")
        .arg(source)
        // Drop chapter tracks. MKVs with chapters make ffmpeg auto-add
        // a `bin_data` text stream that downstream demuxers trip on.
        // `-map_chapters` is an OUTPUT option — must come after `-i`.
        .args(["-map_chapters", "-1"]);

    // Video output. `0:V:0?` skips attached pictures (cover art).
    cmd.args(["-map", "0:V:0?", "-c:v", "copy"]);
    if matches!(
        plan.source_video_codec.as_deref().map(str::to_ascii_lowercase).as_deref(),
        Some("hevc" | "h265")
    ) {
        // Force the `hvc1` MP4 brand — `hev1` (ffmpeg's default for
        // HEVC) puts SPS/PPS in the bitstream and is rejected by
        // browsers, which only decode HEVC when the parameter sets
        // live in the `hvcC` box of the sample entry.
        cmd.args(["-tag:v", "hvc1"]);
    }
    cmd.args(["-an", "-sn", "-dn", "-f", "mp4"]).arg(video_tmp);

    // One MP4 per audio rendition.
    for (a, tmp) in plan.audio.iter().zip(audio_tmps.iter()) {
        cmd.args(["-map", &format!("0:a:{}", a.source_idx)]);
        match a.codec {
            AudioCodec::Copy => {
                cmd.args(["-c:a", "copy"]);
            }
            AudioCodec::Aac => {
                // Stereo AAC 192 k — small enough that we always emit
                // it for non-AAC sources (DTS / AC-3 / FLAC etc.).
                cmd.args(["-c:a", "aac", "-b:a", "192k", "-ac", "2"]);
            }
        }
        // Each output has a single audio stream so `0:a:0` IS the one
        // we just mapped. Re-state the language so it survives the
        // shaka pipeline (even with `-c copy`, isolating the stream
        // can drop the tag).
        cmd.args([
            "-vn",
            "-sn",
            "-dn",
            "-metadata:s:a:0",
            &format!("language={}", a.language),
            "-f",
            "mp4",
        ])
        .arg(tmp);
    }

    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());

    tracing::info!(
        source = %source.display(),
        renditions = plan.audio.len() + 1,
        "remuxer: spawning ffmpeg",
    );

    let mut child = cmd.spawn()?;
    if let Some(stderr) = child.stderr.take() {
        let log = log_path.to_path_buf();
        tokio::spawn(async move { drain_stderr_to_log(stderr, log).await });
    }
    let status = child.wait().await?;
    if !status.success() {
        return Err(RemuxError::Failed(
            status.code().unwrap_or(-1),
            log_path.display().to_string(),
        ));
    }
    Ok(())
}

async fn run_shaka(
    out_dir: &Path,
    video_tmp: &Path,
    audio_tmps: &[PathBuf],
    plan: &RemuxPlan,
    log_path: &Path,
) -> Result<(), RemuxError> {
    // shaka uses `,`-separated key=value pairs PER input. Embed paths
    // verbatim — `OsStr::display()` is fine here because we control
    // every component of the path (no quoting needed).
    let stream_descriptors: Vec<String> = std::iter::once(format!(
        "in={input},stream=video,init_segment={dir}/{VIDEO_VARIANT}_init.mp4,segment_template={dir}/{VIDEO_VARIANT}_$Number$.m4s,playlist_name={VIDEO_VARIANT}.m3u8",
        input = video_tmp.display(),
        dir = out_dir.display(),
    ))
    .chain(plan.audio.iter().zip(audio_tmps.iter()).map(|(a, tmp)| {
        format!(
            "in={input},stream=audio,language={lang},init_segment={dir}/{name}_init.mp4,segment_template={dir}/{name}_$Number$.m4s,playlist_name={name}.m3u8,hls_group_id=audio,hls_name={name}",
            input = tmp.display(),
            lang = a.language,
            dir = out_dir.display(),
            name = a.name,
        )
    }))
    .collect();

    let mut cmd = Command::new("packager");
    for sd in &stream_descriptors {
        cmd.arg(sd);
    }
    cmd.args(["--segment_duration", "6"])
        .arg("--hls_master_playlist_output")
        .arg(out_dir.join(MASTER_PLAYLIST))
        // Quiet by default — we tee stderr into the same log file as
        // ffmpeg, so when something explodes the operator gets one log
        // per cache entry with both stages' output.
        .args(["--quiet"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    tracing::info!(
        dir = %out_dir.display(),
        renditions = plan.audio.len() + 1,
        "remuxer: spawning shaka-packager",
    );

    let mut child = cmd.spawn()?;
    if let Some(stderr) = child.stderr.take() {
        let log = log_path.to_path_buf();
        tokio::spawn(async move { append_stderr_to_log(stderr, log).await });
    }
    let status = child.wait().await?;
    if !status.success() {
        return Err(RemuxError::Failed(
            status.code().unwrap_or(-1),
            log_path.display().to_string(),
        ));
    }
    Ok(())
}

async fn append_stderr_to_log(stderr: tokio::process::ChildStderr, log_path: PathBuf) {
    let mut reader = tokio::io::BufReader::new(stderr).lines();
    let mut log = tokio::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&log_path)
        .await
        .ok();
    while let Ok(Some(line)) = reader.next_line().await {
        tracing::debug!(target: "shaka-packager", "{line}");
        if let Some(f) = log.as_mut() {
            use tokio::io::AsyncWriteExt;
            let _ = f.write_all(line.as_bytes()).await;
            let _ = f.write_all(b"\n").await;
        }
    }
}

/// Read the last `max_bytes` of `log_path` as a UTF-8 string. Used to
/// extract the actual ffmpeg error message (the tail is where the fatal
/// line lives) and surface it via `PlayStatus` when a remux fails.
async fn read_log_tail(log_path: &Path, max_bytes: usize) -> Option<String> {
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(log_path).await.ok()?;
    let len = file.metadata().await.ok()?.len();
    let max = u64::try_from(max_bytes).unwrap_or(u64::MAX);
    let start = len.saturating_sub(max);
    if start > 0 {
        use tokio::io::AsyncSeekExt;
        file.seek(std::io::SeekFrom::Start(start)).await.ok()?;
    }
    let mut buf = String::new();
    file.read_to_string(&mut buf).await.ok()?;
    Some(buf)
}

async fn drain_stderr_to_log(stderr: tokio::process::ChildStderr, log_path: PathBuf) {
    let mut reader = tokio::io::BufReader::new(stderr).lines();
    let mut log = tokio::fs::File::create(&log_path).await.ok();
    while let Ok(Some(line)) = reader.next_line().await {
        tracing::debug!(target: "ffmpeg-remux", "{line}");
        if let Some(f) = log.as_mut() {
            use tokio::io::AsyncWriteExt;
            let _ = f.write_all(line.as_bytes()).await;
            let _ = f.write_all(b"\n").await;
        }
    }
}

// Suppress dead-code warnings on Child while we don't surface a kill API.
#[allow(dead_code)]
fn _silence_child_unused(_: &Child) {}
