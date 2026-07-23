//! Last-resort server-side deinterlace/transcode for Live TV.
//!
//! Some IPTV restreams are interlaced + reference-frame-corrupt H.264 (M6's
//! only living feed, notably). Browsers' software decoders power through, but
//! TV boxes wedge on them — hardware decoders silently produce no frames and
//! the platform SOFTWARE decoder fails too (verified on-device via the watch
//! screen's staged escalation). When the client has exhausted local options
//! it asks for `/transcode/master.m3u8`: one shared ffmpeg per channel pulls
//! the elected upstream, deinterlaces (yadif) and re-encodes clean progressive
//! H.264 into a rolling HLS window on disk. Mirrors the "only when obligatory"
//! philosophy of the VOD AV1 transcode path — nothing is transcoded until a
//! client proves it can't play the original.
//!
//! Sessions are shared (household-scale: N viewers of one channel = one
//! ffmpeg), capped, and reaped after a short idle window (nobody fetching
//! segments = nobody watching).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::LiveTvError;

/// Runaway backstop, NOT a service limit. Sessions are per-CHANNEL (every
/// viewer of a channel shares one ffmpeg) and only spawn after a client
/// proved it can't decode the original — so real household usage can't
/// plausibly exceed a handful. This cap exists solely so a buggy client
/// retry-loop can't fork-bomb ffmpeg across many channels; a human should
/// never hit it. Sized for two fully-warmed muxes (up to 11 channels on
/// the current grid) plus re-encode headroom.
const MAX_SESSIONS: usize = 16;

// Idle windows before a session is reaped, per mode. PINNED remux
// sessions (prewarm mux members, `-c copy`, ~zero CPU) stay warm for
// hours — the 6 h recycle just refreshes ffmpeg. UNPINNED remux sessions
// are viewer-borrowed muxes: reap them soon after the viewer leaves so
// the adapter frees up for re-pinning. Re-encode burns a core: reap fast.
const IDLE_REAP_REMUX_PINNED: Duration = Duration::from_hours(6);
const IDLE_REAP_REMUX: Duration = Duration::from_mins(15);
const IDLE_REAP_REENCODE: Duration = Duration::from_mins(2);

/// The tuner box has two RF frontends — at most two concurrent
/// frequencies (muxes) regardless of how many channels ride each.
const TUNER_ADAPTERS: usize = 2;

/// A mux is "viewer-hot" when one of its sessions was touched this
/// recently. Real viewers hit the playlist/segment routes every couple
/// of seconds; warm idle sessions are never touched (the prewarm loop
/// skips live sessions precisely so heat stays a viewer-only signal).
/// Longer than any legitimate fetch gap, shorter than the point where a
/// paused live viewer has out-scrolled the rolling window anyway.
const VIEWER_HOT_MS: u64 = 90_000;

/// How long `master_playlist` waits for ffmpeg to produce a playable window
/// (input probe + first segments ≈ 10-20 s on a live source).
const STARTUP_WAIT: Duration = Duration::from_secs(25);
/// Segments the playlist must reference before a master response is
/// considered playable — a client joining a 1-segment window rides the
/// live edge with zero margin and stutters for its first minute. Three
/// segments (~6 s) give it a real starting cushion; warm sessions pass
/// on the first check regardless.
const READY_SEGMENTS: usize = 3;

/// Rolling window kept on disk. 6 × 4 s ≈ 24 s of live buffer.
const HLS_TIME_S: u32 = 2;
const HLS_LIST_SIZE: u32 = 6;
/// Segments kept on disk after rolling off the playlist (~24 s of grace —
/// the web engine's mount works from a playlist snapshot for several
/// seconds and must never race the deletion window).
const HLS_DELETE_THRESHOLD: u32 = 12;

fn epoch_ms() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis()),
    )
    .unwrap_or(0)
}

/// What ffmpeg does to the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Deinterlace + re-encode (the historical last-resort path).
    Reencode,
    /// `-c copy` repackage of an already-clean transport stream into HLS —
    /// used for the household tuner's raw-TS feeds. Near-zero CPU.
    Remux,
}

impl Mode {
    /// Sessions are keyed per (mode, channel) so a client-requested
    /// re-encode never collides with the tuner's remux of the same channel.
    fn key(self, channel_key: &str) -> String {
        match self {
            Self::Reencode => format!("enc:{channel_key}"),
            Self::Remux => format!("mux:{channel_key}"),
        }
    }

    /// Segment filename prefix — mode-specific so both sessions of one
    /// channel can share a single segment route transparently.
    fn seg_prefix(self) -> &'static str {
        match self {
            Self::Reencode => "seg",
            Self::Remux => "mux",
        }
    }

    /// Recover the mode from a segment name minted by [`Self::seg_prefix`].
    pub fn from_segment_name(name: &str) -> Self {
        if name.starts_with("mux") {
            Self::Remux
        } else {
            Self::Reencode
        }
    }
}

struct Session {
    dir: PathBuf,
    child: tokio::sync::Mutex<tokio::process::Child>,
    last_access_ms: AtomicU64,
    mode: Mode,
    /// Tuner mux frequency (`f=` of the tune URL) — `None` for internet
    /// inputs. Drives the mux-admission bookkeeping.
    freq: Option<String>,
    /// Prewarm mux member: kept warm for hours instead of minutes.
    pinned: std::sync::atomic::AtomicBool,
}

impl Session {
    fn touch(&self) {
        self.last_access_ms.store(epoch_ms(), Ordering::Relaxed);
    }

    fn hot(&self) -> bool {
        epoch_ms().saturating_sub(self.last_access_ms.load(Ordering::Relaxed)) < VIEWER_HOT_MS
    }

    fn idle(&self) -> bool {
        let limit = match self.mode {
            Mode::Remux if self.pinned.load(Ordering::Relaxed) => IDLE_REAP_REMUX_PINNED,
            Mode::Remux => IDLE_REAP_REMUX,
            Mode::Reencode => IDLE_REAP_REENCODE,
        };
        epoch_ms().saturating_sub(self.last_access_ms.load(Ordering::Relaxed))
            > u64::try_from(limit.as_millis()).unwrap_or(u64::MAX)
    }
}

#[derive(Default)]
pub struct TranscodeManager {
    sessions: tokio::sync::Mutex<HashMap<String, Arc<Session>>>,
}

impl TranscodeManager {
    /// Serve the current transcoded playlist for a channel, starting the
    /// ffmpeg session if needed. `input` is the channel's ELECTED upstream
    /// (the same feed the plain proxy would serve). `pinned` marks prewarm
    /// mux members (kept warm for hours; see the idle windows above).
    pub async fn master_playlist(
        &self,
        mode: Mode,
        channel_key: &str,
        input: &str,
        user_agent: &str,
        referrer: Option<&str>,
        pinned: bool,
    ) -> Result<String, LiveTvError> {
        let session = self
            .ensure(mode, channel_key, input, user_agent, referrer, pinned)
            .await?;
        session.touch();
        let playlist = session.dir.join("live.m3u8");
        // Wait for a playable WINDOW (see [`READY_SEGMENTS`]).
        let deadline = std::time::Instant::now() + STARTUP_WAIT;
        loop {
            if let Ok(body) = tokio::fs::read_to_string(&playlist).await {
                // (ffmpeg emits lowercase names — see -hls_segment_filename;
                // .ts for re-encode, .m4s for the legacy fMP4 remux.)
                #[allow(clippy::case_sensitive_file_extension_comparisons)]
                let segments = body
                    .lines()
                    .map(str::trim)
                    .filter(|l| l.ends_with(".ts") || l.ends_with(".m4s"))
                    .count();
                if segments >= READY_SEGMENTS {
                    return Ok(body);
                }
            }
            // Bail out early if ffmpeg died (bad input, missing codec…).
            if let Ok(Some(status)) = session.child.lock().await.try_wait() {
                self.remove(&mode.key(channel_key)).await;
                return Err(LiveTvError::Upstream(format!(
                    "live transcoder exited early ({status})"
                )));
            }
            if std::time::Instant::now() >= deadline {
                return Err(LiveTvError::Upstream(
                    "live transcoder did not become ready in time".into(),
                ));
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    }

    /// Serve one transcoded segment. Name is strictly validated — it is a
    /// path element on disk.
    pub async fn segment(
        &self,
        mode: Mode,
        channel_key: &str,
        name: &str,
    ) -> Result<Vec<u8>, LiveTvError> {
        if !is_valid_segment_name(name) {
            return Err(LiveTvError::BadProxyRequest);
        }
        let session = {
            let sessions = self.sessions.lock().await;
            sessions
                .get(&mode.key(channel_key))
                .cloned()
                .ok_or(LiveTvError::UnknownChannel)?
        };
        session.touch();
        tokio::fs::read(session.dir.join(name))
            .await
            .map_err(|e| LiveTvError::Upstream(format!("segment read: {e}")))
    }

    async fn ensure(
        &self,
        mode: Mode,
        channel_key: &str,
        input: &str,
        user_agent: &str,
        referrer: Option<&str>,
        pinned: bool,
    ) -> Result<Arc<Session>, LiveTvError> {
        let session_key = mode.key(channel_key);
        let mut sessions = self.sessions.lock().await;

        // Existing live session → reuse (also for a second household viewer).
        if let Some(existing) = sessions.get(&session_key) {
            if existing
                .child
                .lock()
                .await
                .try_wait()
                .ok()
                .flatten()
                .is_none()
            {
                if pinned {
                    existing.pinned.store(true, Ordering::Relaxed);
                }
                return Ok(existing.clone());
            }
            // ffmpeg died (upstream hiccup) — clean up and respawn below.
            let dead = sessions.remove(&session_key);
            if let Some(dead) = dead {
                let _ = tokio::fs::remove_dir_all(&dead.dir).await;
            }
        }

        if sessions.len() >= MAX_SESSIONS {
            // See MAX_SESSIONS — this is a bug tripwire, log it loudly.
            tracing::warn!(
                sessions = sessions.len(),
                "live transcode session cap hit — likely a client retry loop"
            );
            return Err(LiveTvError::Upstream(
                "live transcoder session cap reached".into(),
            ));
        }

        let dir = std::env::temp_dir()
            .join("iris-livetv")
            .join(session_key.replace([':', '/'], "_"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| LiveTvError::Upstream(format!("transcode dir: {e}")))?;

        let mut cmd = tokio::process::Command::new("ffmpeg");
        cmd.arg("-nostdin")
            .args(["-hide_banner", "-loglevel", "warning"])
            .args(["-user_agent", user_agent]);
        if let Some(referrer) = referrer {
            cmd.args(["-headers", &format!("Referer: {referrer}\r\n")]);
        }
        cmd.args(["-i", input]);
        // Explicit stream mapping. Tuner inputs (tunerd v2) carry the UNION
        // of every concurrent viewer's services in one TS — the service
        // MUST be picked by PID (the tune URL lists them in mux-survey
        // order: PAT, SDT, PMT, video, audio…). Internet feeds keep the
        // positional first-video/first-audio mapping.
        if let Some((vpid, apid)) = tuner_pids(input) {
            cmd.args(["-map", &format!("0:i:{vpid}")])
                .args(["-map", &format!("0:i:{apid}?")]);
        } else {
            cmd.args(["-map", "0:v:0", "-map", "0:a:0?"]);
        }
        match mode {
            Mode::Reencode => {
                // Deinterlace to 25p + clamp to 720 lines: the whole point
                // is a stream every hardware decoder eats; 1080p50 output
                // would just trade a decode wedge for an encode/decode CPU
                // wall.
                cmd.args(["-vf", "yadif=0:-1:0,scale=-2:720"])
                    .args(["-c:v", "libx264", "-preset", "veryfast"])
                    .args(["-crf", "23"])
                    .args(["-g", "50", "-sc_threshold", "0"])
                    .args(["-pix_fmt", "yuv420p"])
                    // Audio passes through — eac3 was never the problem (the
                    // TV's ffmpeg extension decodes it fine in VOD).
                    .args(["-c:a", "copy"]);
            }
            Mode::Remux => {
                // The tuner's TS is clean broadcast H.264 — repackage only,
                // `-c copy` for BOTH streams into **MPEG-TS segments**. Audio
                // stays broadcast E-AC-3/AC-3: the TV decodes it natively and
                // the web player consumes this feed through the mediabunny
                // live engine (client decode), NOT through hls.js.
                //
                // NOT fMP4 — that was tried and is a measured trap for this
                // stream: a live join starts video 0.5-1.3 s after audio
                // (leading video is dropped until the first keyframe), and
                // ffmpeg's mp4 muxer encodes that offset as a video EDIT
                // LIST, which fragmented-MP4 consumers (mediabunny, VLC,
                // MSE) ignore — video then presents early by exactly the
                // join offset (the live A/V desync saga of 2026-07-23). TS
                // segments carry absolute PTS shared by both tracks:
                // alignment is intrinsic, nothing to interpret.
                cmd.args(["-c", "copy"]);
            }
        }
        cmd.args(["-f", "hls"])
            .args(["-hls_time", &HLS_TIME_S.to_string()])
            .args(["-hls_list_size", &HLS_LIST_SIZE.to_string()])
            .args(["-hls_delete_threshold", &HLS_DELETE_THRESHOLD.to_string()])
            // program_date_time: the web player's E-AC-3 WebAudio sidecar
            // (used on re-encoded feeds whose audio stays E-AC-3) syncs audio
            // to video through EXT-X-PROGRAM-DATE-TIME — without the tags the
            // sidecar has no clock and those streams play mute.
            .args([
                "-hls_flags",
                "delete_segments+independent_segments+program_date_time",
            ])
            .arg("-hls_segment_filename")
            .arg(dir.join(format!("{}%06d.ts", mode.seg_prefix())))
            .arg(dir.join("live.m3u8"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            // The reaper kills idle sessions; kill_on_drop covers server
            // shutdown so no orphan encoder outlives the process.
            .kill_on_drop(true);

        let child = cmd
            .spawn()
            .map_err(|e| LiveTvError::Upstream(format!("spawn ffmpeg: {e}")))?;
        tracing::info!(channel = %channel_key, ?mode, "live transcode session started");

        let session = Arc::new(Session {
            dir,
            child: tokio::sync::Mutex::new(child),
            last_access_ms: AtomicU64::new(epoch_ms()),
            mode,
            freq: (mode == Mode::Remux).then(|| tuner_freq(input)).flatten(),
            pinned: std::sync::atomic::AtomicBool::new(pinned),
        });
        sessions.insert(session_key, session.clone());
        Ok(session)
    }

    async fn remove(&self, channel_key: &str) {
        let session = self.sessions.lock().await.remove(channel_key);
        if let Some(session) = session {
            let _ = session.child.lock().await.kill().await;
            let _ = tokio::fs::remove_dir_all(&session.dir).await;
        }
    }

    /// Kill sessions nobody has touched within their per-mode idle window
    /// service's background loop.
    pub async fn reap_idle(&self) {
        let idle_keys: Vec<String> = {
            let sessions = self.sessions.lock().await;
            sessions
                .iter()
                .filter(|(_, s)| s.idle())
                .map(|(k, _)| k.clone())
                .collect()
        };
        for key in idle_keys {
            tracing::info!(channel = %key, "reaping idle live transcode session");
            self.remove(&key).await;
        }
    }

    /// Whether a live (ffmpeg still running) session exists for the key.
    /// Deliberately does NOT touch — the prewarm loop uses this so viewer
    /// heat stays a viewer-only signal.
    pub async fn is_warm(&self, mode: Mode, channel_key: &str) -> bool {
        let session = {
            let sessions = self.sessions.lock().await;
            sessions.get(&mode.key(channel_key)).cloned()
        };
        match session {
            Some(s) => s.child.lock().await.try_wait().ok().flatten().is_none(),
            None => false,
        }
    }

    /// Tuner frequencies with at least one session, and whether any of
    /// that mux's sessions is viewer-hot (see [`VIEWER_HOT_MS`]).
    async fn freq_heat(&self) -> HashMap<String, bool> {
        let sessions = self.sessions.lock().await;
        let mut heat: HashMap<String, bool> = HashMap::new();
        for s in sessions.values() {
            if let Some(f) = &s.freq {
                *heat.entry(f.clone()).or_insert(false) |= s.hot();
            }
        }
        heat
    }

    /// Mux admission: the tuner has [`TUNER_ADAPTERS`] RF frontends, so at
    /// most that many concurrent frequencies. Joining an already-tuned mux
    /// or a free adapter is always granted. A request for one mux beyond
    /// capacity reclaims a mux NOBODY is watching (its warm sessions are
    /// expendable — killing them frees the adapter, tunerd then hands it
    /// to the new tune). When every tuned mux has active viewers the
    /// request is refused: the caller falls back to internet tiers instead
    /// of cutting someone else's stream.
    pub async fn admit_mux(&self, freq: &str) -> bool {
        let heat = self.freq_heat().await;
        if heat.contains_key(freq) || heat.len() < TUNER_ADAPTERS {
            return true;
        }
        let victim = heat.iter().find_map(|(f, hot)| (!hot).then(|| f.clone()));
        match victim {
            Some(v) => {
                tracing::info!(
                    freq = %freq,
                    victim = %v,
                    "reclaiming unwatched tuner mux for a viewer"
                );
                self.reap_freq(&v).await;
                true
            }
            None => false,
        }
    }

    /// Non-mutating preview of [`Self::admit_mux`] — lets the prewarm loop
    /// skip channels quietly instead of electing (and fetching) an
    /// internet source for a channel it can't warm anyway.
    pub async fn mux_available(&self, freq: &str) -> bool {
        let heat = self.freq_heat().await;
        heat.contains_key(freq) || heat.len() < TUNER_ADAPTERS || heat.values().any(|hot| !hot)
    }

    /// Kill every session on a mux (all its channels share the adapter).
    async fn reap_freq(&self, freq: &str) {
        let keys: Vec<String> = {
            let sessions = self.sessions.lock().await;
            sessions
                .iter()
                .filter(|(_, s)| s.freq.as_deref() == Some(freq))
                .map(|(k, _)| k.clone())
                .collect()
        };
        for key in keys {
            tracing::info!(channel = %key, freq = %freq, "reaping session (mux reclaimed)");
            self.remove(&key).await;
        }
    }
}

/// Mux frequency (`f=` query param) of a tunerd `/tune` URL — the unit of
/// adapter contention. `None` when the input isn't a tune URL.
pub fn tuner_freq(input: &str) -> Option<String> {
    if !input.contains("/tune?") {
        return None;
    }
    input
        .split("f=")
        .nth(1)
        .map(|rest| rest.split('&').next().unwrap_or_default().to_string())
        .filter(|f| !f.is_empty())
}

/// The tuner URL carries the mux-survey pid list in grid order
/// ([PAT, SDT, PMT, video, audio…]). Returns `(video_pid, audio_pid)` when
/// the input looks like a tunerd `/tune` URL with at least those five
/// entries, `None` for internet feeds.
fn tuner_pids(input: &str) -> Option<(u16, u16)> {
    let list: Vec<u16> = input
        .split("pids=")
        .nth(1)?
        .split('&')
        .next()?
        .split(',')
        .filter_map(|p| {
            let p = p.trim();
            p.strip_prefix("0x")
                .map_or_else(|| p.parse().ok(), |hex| u16::from_str_radix(hex, 16).ok())
        })
        .collect();
    (list.len() >= 5).then(|| (list[3], list[4]))
}

/// `seg000042.ts` / `mux000042.m4s` / `muxinit.mp4` and nothing else — this
/// becomes a filename under the session dir, so it must never traverse.
fn is_valid_segment_name(name: &str) -> bool {
    if name == "muxinit.mp4" {
        return true;
    }
    name.strip_prefix("seg")
        .or_else(|| name.strip_prefix("mux"))
        .and_then(|rest| {
            rest.strip_suffix(".ts")
                .or_else(|| rest.strip_suffix(".m4s"))
        })
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_name_validation() {
        assert!(is_valid_segment_name("seg000001.ts"));
        assert!(is_valid_segment_name("seg9.ts"));
        assert!(is_valid_segment_name("mux000001.ts"));
        assert!(!is_valid_segment_name("mux.ts"));
        assert!(!is_valid_segment_name("seg.ts"));
        assert!(!is_valid_segment_name("../etc/passwd"));
        assert!(!is_valid_segment_name("seg00/1.ts"));
        assert!(!is_valid_segment_name("seg1.ts.tmp"));
        assert!(!is_valid_segment_name("live.m3u8"));
    }

    #[test]
    fn mode_roundtrips_through_segment_names() {
        assert_eq!(Mode::from_segment_name("mux000042.ts"), Mode::Remux);
        assert_eq!(Mode::from_segment_name("seg000042.ts"), Mode::Reencode);
    }

    #[test]
    fn tuner_freq_parses_tune_urls_only() {
        assert_eq!(
            tuner_freq("http://box:8554/tune?a=0&f=506000000&pids=0x0,0x11"),
            Some("506000000".to_string())
        );
        assert_eq!(
            tuner_freq("http://box:8554/tune?f=690000000"),
            Some("690000000".to_string())
        );
        assert_eq!(tuner_freq("http://box:8554/tune?a=0&pids=0x0"), None);
        assert_eq!(tuner_freq("https://cdn.example.com/live.m3u8?f=1"), None);
    }
}
