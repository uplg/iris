//! Persistent ffmpeg HLS muxer with **multi-audio rendition** support.
//!
//! Each `(infohash, file_idx)` key spawns a single ffmpeg process that
//! produces:
//!
//!   * one **video-only** HLS variant under `stream_0/playlist.m3u8`
//!   * **N audio-only** variants under `stream_{1..=N}/playlist.m3u8`,
//!     one per audio track from the source
//!   * a **master playlist** at `master.m3u8` that wires them together via
//!     `#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud"` rules
//!
//! Players (hls.js, ExoPlayer/Media3, Safari) load the master, expose every
//! audio rendition through their built-in track selector, and switching
//! happens **without** re-fetching the video segments — they just stop
//! pulling `stream_1/seg_*.m4s` and start pulling `stream_2/seg_*.m4s`. No
//! re-segmentation, no re-buffer, no double disk usage.
//!
//! On-demand single-segment generation was tried and **abandoned**: with
//! `-c:v copy` each independent ffmpeg call has slightly off PTS (because
//! `-ss BEFORE -i` snaps to the previous keyframe), making players see the
//! timeline rewind on every seek. Letting the muxer produce the whole
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
    #[error("source has no streams to mux")]
    NoStreams,
}

/// One audio rendition in the source. We pass these into [`HlsManager::ensure_job`]
/// so the muxer knows which `0:a:N` mappings to emit and how to label them in
/// the master playlist (`LANGUAGE=...,NAME=...,DEFAULT=...`).
#[derive(Debug, Clone)]
pub struct AudioTrack {
    /// Index in ffprobe's `audio` array (the ffmpeg `0:a:N` selector).
    pub track_idx: u32,
    /// Codec from ffprobe (`aac`, `eac3`, `dts`…) — used to decide
    /// `copy` vs transcode-to-AAC.
    pub codec: String,
    /// ISO-639 language tag if known.
    pub language: Option<String>,
    /// Human-readable name (typically from the source's stream `title`).
    pub name: Option<String>,
    /// True for the source's default track. Becomes `DEFAULT=YES` in the
    /// master playlist.
    pub default: bool,
}

/// Names of the artifacts ffmpeg writes. Centralised so the route layer and
/// the manager agree on filesystem layout.
pub const MASTER_PLAYLIST: &str = "master.m3u8";
pub const VIDEO_VARIANT_DIR: &str = "stream_0";
pub const VIDEO_VARIANT_PLAYLIST: &str = "stream_0/playlist.m3u8";

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

    /// Read the master playlist immediately. We hand-write it at job spawn
    /// time so it's always on disk before any client request. The variant
    /// playlists referenced by the master use `#EXT-X-PLAYLIST-TYPE:EVENT`
    /// — players load them, see whatever segments ffmpeg has produced so
    /// far, and re-poll as more arrive. ENDLIST is appended on clean exit.
    /// This means playback can start within seconds of clicking Play
    /// rather than waiting for the entire pre-segmentation pass.
    pub async fn read_master_playlist(
        &self,
        key: &str,
        _max_wait: Duration,
    ) -> Result<String, HlsError> {
        let dir = self.segment_dir_for(key);
        let master = dir.join(MASTER_PLAYLIST);
        tokio::fs::read_to_string(&master)
            .await
            .map_err(|_| HlsError::Timeout(Duration::from_secs(0), master.display().to_string()))
    }

    /// Ensure an ffmpeg HLS job is running for `key`; returns the directory
    /// once spawned (or already running). Segments arrive on disk as ffmpeg
    /// processes the file.
    pub async fn ensure_job(
        &self,
        key: &str,
        source: &Path,
        audio_tracks: &[AudioTrack],
    ) -> Result<PathBuf, HlsError> {
        let dir = self.segment_dir_for(key);

        // Lock-free fast path: when ENDLIST is on disk we KNOW there's
        // nothing to spawn, so don't even contend the jobs Mutex. This is
        // critical for serving — hls.js fetches 5-10 segments in parallel
        // and each segment request used to acquire the Mutex + do sync
        // I/O inside the lock. Median segment latency went from <5 ms to
        // hundreds of ms under load.
        if is_done(&dir).await {
            return Ok(dir);
        }
        // Fast path 2: ffmpeg ran to completion at least once but ENDLIST
        // somehow didn't land — fix it up & return without locking.
        if tokio::fs::try_exists(dir.join(".done")).await.unwrap_or(false)
            && tokio::fs::try_exists(dir.join(MASTER_PLAYLIST)).await.unwrap_or(false)
        {
            fixup_endlist(&dir, audio_tracks.len());
            return Ok(dir);
        }

        let mut jobs = self.inner.jobs.lock().await;
        if let Some(j) = jobs.get(key) {
            return Ok(j.dir.clone());
        }
        // Re-check the same disk-state guards we did lock-free above —
        // another task may have completed the job between our check and
        // the lock acquisition.
        if is_done(&dir).await {
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
        // ffmpeg's HLS muxer doesn't create parent directories for the
        // segment pattern (`stream_%v/seg_%05d.m4s`), so we pre-create them.
        // One dir for video + one per audio track.
        for i in 0..=audio_tracks.len() {
            std::fs::create_dir_all(dir.join(format!("stream_{i}")))?;
        }
        // Hand-write the master playlist *before* spawning ffmpeg so the
        // path-based idempotency check (`dir/master.m3u8` exists) works
        // immediately on the next request. ffmpeg's own `-master_pl_name`
        // historically writes the master inside the first variant's dir
        // (`stream_0/master.m3u8`), which our check misses → infinite
        // respawn loop. Owning the master here also gives us full control
        // over LANGUAGE / NAME / DEFAULT attributes.
        std::fs::write(dir.join(MASTER_PLAYLIST), build_master_playlist(audio_tracks))?;
        let child = spawn_ffmpeg(source, &dir, audio_tracks)?;

        let key_owned = key.to_string();
        let jobs_handle = self.inner.jobs.clone();
        let job_for_reaper = j.clone();
        let audio_count = audio_tracks.len();
        tokio::spawn(async move {
            let exit = await_child_exit(child).await;
            match exit {
                Ok(status) if status.success() => {
                    tracing::info!(key = %key_owned, "ffmpeg HLS job finished");
                    // Sentinel: prevents respawn after a clean exit even
                    // if ffmpeg forgot to write ENDLIST. Without this
                    // marker `ensure_job` would loop forever on sources
                    // that trigger that ffmpeg quirk.
                    let _ = std::fs::write(job_for_reaper.dir.join(".done"), "");
                    fixup_endlist(&job_for_reaper.dir, audio_count);
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

    /// Whether an ffmpeg job is currently registered for `key`. The reaper
    /// removes the entry on exit, so this returns `false` once ffmpeg is
    /// done — even if segments are still on disk.
    pub async fn is_job_active(&self, key: &str) -> bool {
        self.inner.jobs.lock().await.contains_key(key)
    }

    /// Sweep `base_dir` and remove HLS directories whose video variant
    /// playlist hasn't been touched in the last `max_age`. The mtime is
    /// updated by the temp_file rename every time ffmpeg writes a new
    /// segment, so it also reflects "last actively watched" — the player
    /// triggers ffmpeg to generate segments, which bumps mtime.
    ///
    /// HLS data is a *cache* (a re-wrap of the source bytes), so throwing
    /// it away is free: at next Play the master.m3u8 endpoint kicks ffmpeg
    /// again and segments regenerate. Currently-running jobs are skipped so
    /// we never pull data out from under a viewer.
    ///
    /// Returns the number of directories removed.
    pub async fn evict_idle(&self, max_age: std::time::Duration) -> usize {
        let cutoff = match std::time::SystemTime::now().checked_sub(max_age) {
            Some(c) => c,
            None => return 0,
        };
        let active: std::collections::HashSet<String> = self
            .inner
            .jobs
            .lock()
            .await
            .keys()
            .cloned()
            .collect();
        let read = match std::fs::read_dir(&self.inner.base_dir) {
            Ok(r) => r,
            Err(_) => return 0,
        };
        let mut evicted = 0usize;
        for entry in read.flatten() {
            let path = entry.path();
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if active.contains(&name) {
                continue;
            }
            // Prefer the video variant playlist's mtime — it's bumped on
            // each segment write, so it tracks "last frame served" closely.
            let candidate_mtime = std::fs::metadata(path.join(VIDEO_VARIANT_PLAYLIST))
                .and_then(|m| m.modified())
                .or_else(|_| std::fs::metadata(path.join(MASTER_PLAYLIST)).and_then(|m| m.modified()))
                .or_else(|_| std::fs::metadata(&path).and_then(|m| m.modified()));
            let Ok(mtime) = candidate_mtime else { continue };
            if mtime > cutoff {
                continue;
            }
            match tokio::fs::remove_dir_all(&path).await {
                Ok(()) => {
                    evicted += 1;
                    tracing::info!(key = %name, "hls: evicted idle cache");
                }
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "hls evict_idle failed");
                }
            }
        }
        evicted
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

    /// Count `.m4s` segments in the **video** variant directory. Used by the
    /// progress UI: we expose this as the numerator of "X / ~Y segments".
    pub async fn video_segment_count(&self, key: &str) -> u32 {
        let dir = self.segment_dir_for(key).join(VIDEO_VARIANT_DIR);
        let mut n = 0u32;
        if let Ok(mut rd) = tokio::fs::read_dir(&dir).await {
            while let Ok(Some(e)) = rd.next_entry().await {
                if let Some(name) = e.file_name().to_str() {
                    if name.starts_with("seg_") && name.ends_with(".m4s") {
                        n += 1;
                    }
                }
            }
        }
        n
    }

    /// Read the video variant playlist's ENDLIST flag.
    pub async fn video_endlist_present(&self, key: &str) -> bool {
        let path = self.segment_dir_for(key).join(VIDEO_VARIANT_PLAYLIST);
        tokio::fs::read_to_string(&path)
            .await
            .map(|c| c.contains("#EXT-X-ENDLIST"))
            .unwrap_or(false)
    }
}

fn spawn_ffmpeg(
    source: &Path,
    dir: &Path,
    audio_tracks: &[AudioTrack],
) -> Result<Child, HlsError> {
    if audio_tracks.is_empty() {
        // Some sources are video-only (silent films, screen recordings…);
        // we still produce a single video variant in that case.
    }
    let segment_pattern = dir.join("stream_%v").join("seg_%05d.m4s");
    let variant_playlist = dir.join("stream_%v").join("playlist.m3u8");

    let mut cmd = Command::new("ffmpeg");
    // `-loglevel error` cuts the chatter — `Packet duration: -16` and
    // `Could not find codec parameters for stream X (Subtitle: ...)` are
    // both expected on h264+ass MKVs and not indicative of problems.
    cmd.args(["-hide_banner", "-loglevel", "error", "-y"])
        .arg("-i")
        .arg(source)
        // Video: copy. We never re-encode video here — that's a different
        // subsystem (transcode jobs) used only when the source codec isn't
        // browser-compatible.
        .args(["-map", "0:v:0", "-c:v", "copy"]);

    // Map every audio track in the order we'll declare them in
    // `var_stream_map`. The output stream index is implicit (0..N).
    for t in audio_tracks {
        cmd.args(["-map", &format!("0:a:{}", t.track_idx)]);
    }

    // Audio codec is global, not per-stream: stream-spec args like
    // `-c:a:N copy` proved unreliable across ffmpeg versions when combined
    // with `var_stream_map` (silent exit with code 254 in 7.x). If every
    // selected track is already browser-friendly we copy; otherwise we
    // re-encode all audio to stereo AAC. Mixed-codec sources are rare
    // enough that always-transcoding the lot is fine.
    let all_copyable = !audio_tracks.is_empty()
        && audio_tracks
            .iter()
            .all(|t| matches!(t.codec.to_ascii_lowercase().as_str(), "aac" | "mp3"));
    if all_copyable {
        cmd.args(["-c:a", "copy"]);
    } else if !audio_tracks.is_empty() {
        cmd.args(["-c:a", "aac", "-ac", "2", "-b:a", "192k"]);
    }

    // Drop any other streams (subtitles, data) — subtitles are served via
    // the dedicated subtitle endpoint as WebVTT.
    cmd.args(["-sn", "-dn"]);

    // var_stream_map: declare one variant per output (1 video + N audios)
    // sharing a single audio group. We deliberately avoid `name:` here —
    // when set, ffmpeg substitutes `%v` in `-hls_segment_filename` with
    // that name *instead* of the variant index. Two audio tracks sharing
    // the same `name:` (common when the MKV titles read "Mono" / "Stereo"
    // for both) then collide on the same `stream_<name>/` directory and
    // one overwrites the other. By omitting `name:`, %v stays as the
    // numeric index → stream_0, stream_1, stream_2 as we expect.
    //
    // `language:`, `default:` and the user-facing NAME live in our
    // hand-rolled master.m3u8 instead, so ffmpeg's view of the streams
    // is intentionally minimal.
    let mut map = String::from("v:0");
    if !audio_tracks.is_empty() {
        map.push_str(",agroup:aud");
        for i in 0..audio_tracks.len() {
            map.push(' ');
            map.push_str(&format!("a:{i},agroup:aud"));
        }
    }
    cmd.args(["-var_stream_map", &map]);
    let _ = sanitize; // keep the helper around — used by build_master_playlist.

    // No `-master_pl_name` here — we write the master playlist ourselves
    // in [`HlsManager::ensure_job`] *before* spawning ffmpeg. ffmpeg used
    // to scatter the master inside `stream_0/`, breaking our idempotency
    // check and causing endless respawns when status polls came in after
    // ffmpeg's clean exit.
    cmd.args(["-hls_time", "6"])
        .args(["-hls_list_size", "0"])
        // fMP4 segments: universal container that supports H.264, HEVC,
        // AV1 and VP9 alike. MPEG-TS cannot carry AV1 in a way browsers
        // can decode, so a `... AV1` source plays audio-only there.
        .args(["-hls_segment_type", "fmp4"])
        .args(["-hls_fmp4_init_filename", "init.mp4"])
        .args(["-hls_flags", "temp_file+independent_segments"])
        .arg("-hls_segment_filename")
        .arg(&segment_pattern)
        // `event` writes the playlist incrementally, segment-by-segment, and
        // appends EXT-X-ENDLIST when ffmpeg exits cleanly. With `vod` the
        // playlist would buffer until the end of the file, which on a
        // multi-GB source means clients see no playlist for 30+s.
        .args(["-hls_playlist_type", "event"])
        .args(["-f", "hls"])
        .arg(&variant_playlist)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    tracing::info!(
        source = %source.display(),
        dir = %dir.display(),
        audio_count = audio_tracks.len(),
        var_stream_map = %map,
        "spawning multi-audio ffmpeg HLS job"
    );

    let mut child = cmd.spawn()?;
    if let Some(mut stderr) = child.stderr.take() {
        // Persist ffmpeg's stderr to `dir/ffmpeg.log` for post-mortem of
        // failed runs. Logged to tracing at `debug` only — the live
        // tracing channel was getting spammed with mux-level warnings
        // that aren't actionable.
        let log_path = dir.join("ffmpeg.log");
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut log = tokio::fs::File::create(&log_path).await.ok();
            let mut reader = tokio::io::BufReader::new(&mut stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                tracing::debug!(target: "ffmpeg-hls", "{line}");
                if let Some(f) = log.as_mut() {
                    use tokio::io::AsyncWriteExt;
                    let _ = f.write_all(line.as_bytes()).await;
                    let _ = f.write_all(b"\n").await;
                }
            }
        });
    }
    Ok(child)
}

/// Strip characters ffmpeg's var_stream_map parser dislikes (whitespace,
/// commas, quotes, colons) and keep the result short. Returns `None` if the
/// input is empty/None or sanitises to nothing.
fn sanitize(s: &Option<String>) -> Option<String> {
    let raw = s.as_deref()?;
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(32)
        .collect();
    if cleaned.is_empty() { None } else { Some(cleaned) }
}

/// Build the master playlist content statically. Each audio track in
/// [`audio_tracks`] gets one `EXT-X-MEDIA:TYPE=AUDIO` rendition pointing
/// at `stream_{i+1}/playlist.m3u8`; the lone video variant references
/// the same audio group via `AUDIO="aud"`.
///
/// `CODECS=` is approximate (`avc1.640028,mp4a.40.2`). Both hls.js and
/// ExoPlayer/Media3 accept the manifest with rough codec hints — they
/// re-probe the actual codec from the segment's `init.mp4` anyway.
fn build_master_playlist(audio_tracks: &[AudioTrack]) -> String {
    let mut out = String::from("#EXTM3U\n#EXT-X-VERSION:6\n");
    for (i, t) in audio_tracks.iter().enumerate() {
        let stream_idx = i + 1; // stream_0 is the video variant
        let language = sanitize(&t.language).unwrap_or_else(|| "und".to_string());
        let name = sanitize(&t.name)
            .or_else(|| sanitize(&t.language))
            .unwrap_or_else(|| format!("Track{i}"));
        let default = if t.default { "YES" } else { "NO" };
        out.push_str(&format!(
            "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"{name}\",\
             LANGUAGE=\"{language}\",DEFAULT={default},AUTOSELECT=YES,\
             URI=\"stream_{stream_idx}/playlist.m3u8\"\n"
        ));
    }
    let codecs = if audio_tracks.is_empty() {
        "avc1.640028".to_string()
    } else {
        // H.264 High@4.0 + AAC-LC stereo — the lowest-common-denominator
        // hint that all browsers accept. The actual codec is read from
        // the `init.mp4`, so even AV1/HEVC sources play.
        "avc1.640028,mp4a.40.2".to_string()
    };
    if audio_tracks.is_empty() {
        out.push_str(&format!(
            "#EXT-X-STREAM-INF:BANDWIDTH=2000000,CODECS=\"{codecs}\"\nstream_0/playlist.m3u8\n"
        ));
    } else {
        out.push_str(&format!(
            "#EXT-X-STREAM-INF:BANDWIDTH=2000000,CODECS=\"{codecs}\",AUDIO=\"aud\"\n\
             stream_0/playlist.m3u8\n"
        ));
    }
    out
}

async fn await_child_exit(mut child: Child) -> std::io::Result<std::process::ExitStatus> {
    child.wait().await
}

/// True when the video variant playlist exists on disk and already has
/// `#EXT-X-ENDLIST`. Async, no Mutex needed — the only signal we need to
/// know it's safe to serve segments without involving the job manager.
async fn is_done(dir: &Path) -> bool {
    if !tokio::fs::try_exists(dir.join(MASTER_PLAYLIST)).await.unwrap_or(false) {
        return false;
    }
    let video_pl = dir.join(VIDEO_VARIANT_PLAYLIST);
    match tokio::fs::read_to_string(&video_pl).await {
        Ok(c) => c.contains("#EXT-X-ENDLIST"),
        Err(_) => false,
    }
}

/// Public, async sibling of [`is_done`] used by the route layer to decide
/// whether a segment request can bypass the probe+lock path entirely.
pub async fn is_master_or_endlist_done(dir: &Path) -> bool {
    is_done(dir).await
}

/// Append `#EXT-X-ENDLIST` to every variant playlist that's missing it.
/// `-hls_playlist_type event` is supposed to add this on clean exit, but
/// ffmpeg 7.x occasionally skips it for sub-playlists in multi-variant
/// mode. Without ENDLIST players treat the source as live and lock the
/// timeline at the live edge — no seeking. The append is idempotent.
fn fixup_endlist(dir: &Path, audio_count: usize) {
    for i in 0..=audio_count {
        let path = dir.join(format!("stream_{i}")).join("playlist.m3u8");
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        if content.contains("#EXT-X-ENDLIST") {
            continue;
        }
        let patched = if content.ends_with('\n') {
            format!("{content}#EXT-X-ENDLIST\n")
        } else {
            format!("{content}\n#EXT-X-ENDLIST\n")
        };
        if let Err(e) = std::fs::write(&path, patched) {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "fixup_endlist: could not patch playlist",
            );
        }
    }
}
