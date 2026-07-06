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
/// never hit it.
const MAX_SESSIONS: usize = 10;

/// Kill a session this long after its last playlist/segment request.
const IDLE_REAP: Duration = Duration::from_mins(1);

/// How long `master_playlist` waits for ffmpeg to produce a playable window
/// (input probe + first segments ≈ 10-20 s on a live source).
const STARTUP_WAIT: Duration = Duration::from_secs(25);

/// Rolling window kept on disk. 6 × 4 s ≈ 24 s of live buffer.
const HLS_TIME_S: u32 = 4;
const HLS_LIST_SIZE: u32 = 6;

fn epoch_ms() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis()),
    )
    .unwrap_or(0)
}

struct Session {
    dir: PathBuf,
    child: tokio::sync::Mutex<tokio::process::Child>,
    last_access_ms: AtomicU64,
}

impl Session {
    fn touch(&self) {
        self.last_access_ms.store(epoch_ms(), Ordering::Relaxed);
    }

    fn idle(&self) -> bool {
        epoch_ms().saturating_sub(self.last_access_ms.load(Ordering::Relaxed))
            > u64::try_from(IDLE_REAP.as_millis()).unwrap_or(u64::MAX)
    }
}

#[derive(Default)]
pub struct TranscodeManager {
    sessions: tokio::sync::Mutex<HashMap<String, Arc<Session>>>,
}

impl TranscodeManager {
    /// Serve the current transcoded playlist for a channel, starting the
    /// ffmpeg session if needed. `input` is the channel's ELECTED upstream
    /// (the same feed the plain proxy would serve).
    pub async fn master_playlist(
        &self,
        channel_key: &str,
        input: &str,
        user_agent: &str,
        referrer: Option<&str>,
    ) -> Result<String, LiveTvError> {
        let session = self
            .ensure(channel_key, input, user_agent, referrer)
            .await?;
        session.touch();
        let playlist = session.dir.join("live.m3u8");
        // Wait for ffmpeg to emit a playable window (playlist + segments).
        let deadline = std::time::Instant::now() + STARTUP_WAIT;
        loop {
            if let Ok(body) = tokio::fs::read_to_string(&playlist).await {
                // A playlist that references at least one segment is playable.
                // (ffmpeg emits lowercase names — see -hls_segment_filename.)
                #[allow(clippy::case_sensitive_file_extension_comparisons)]
                if body.lines().any(|l| l.trim().ends_with(".ts")) {
                    return Ok(body);
                }
            }
            // Bail out early if ffmpeg died (bad input, missing codec…).
            if let Ok(Some(status)) = session.child.lock().await.try_wait() {
                self.remove(channel_key).await;
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
    pub async fn segment(&self, channel_key: &str, name: &str) -> Result<Vec<u8>, LiveTvError> {
        if !is_valid_segment_name(name) {
            return Err(LiveTvError::BadProxyRequest);
        }
        let session = {
            let sessions = self.sessions.lock().await;
            sessions
                .get(channel_key)
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
        channel_key: &str,
        input: &str,
        user_agent: &str,
        referrer: Option<&str>,
    ) -> Result<Arc<Session>, LiveTvError> {
        let mut sessions = self.sessions.lock().await;

        // Existing live session → reuse (also for a second household viewer).
        if let Some(existing) = sessions.get(channel_key) {
            if existing
                .child
                .lock()
                .await
                .try_wait()
                .ok()
                .flatten()
                .is_none()
            {
                return Ok(existing.clone());
            }
            // ffmpeg died (upstream hiccup) — clean up and respawn below.
            let dead = sessions.remove(channel_key);
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
            .join(channel_key.replace([':', '/'], "_"));
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
        cmd.args(["-i", input])
            // Deinterlace to 25p + clamp to 720 lines: the whole point is a
            // stream every hardware decoder eats; 1080p50 output would just
            // trade a decode wedge for an encode/decode CPU wall.
            .args(["-vf", "yadif=0:-1:0,scale=-2:720"])
            .args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "23"])
            .args(["-g", "50", "-sc_threshold", "0", "-pix_fmt", "yuv420p"])
            // Audio passes through — eac3 was never the problem (the TV's
            // ffmpeg extension decodes it fine in VOD).
            .args(["-c:a", "copy"])
            .args(["-f", "hls"])
            .args(["-hls_time", &HLS_TIME_S.to_string()])
            .args(["-hls_list_size", &HLS_LIST_SIZE.to_string()])
            .args(["-hls_flags", "delete_segments+independent_segments"])
            .arg("-hls_segment_filename")
            .arg(dir.join("seg%06d.ts"))
            .arg(dir.join("live.m3u8"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            // The reaper kills idle sessions; kill_on_drop covers server
            // shutdown so no orphan encoder outlives the process.
            .kill_on_drop(true);

        let child = cmd
            .spawn()
            .map_err(|e| LiveTvError::Upstream(format!("spawn ffmpeg: {e}")))?;
        tracing::info!(channel = %channel_key, input = %input, "live transcode session started");

        let session = Arc::new(Session {
            dir,
            child: tokio::sync::Mutex::new(child),
            last_access_ms: AtomicU64::new(epoch_ms()),
        });
        sessions.insert(channel_key.to_string(), session.clone());
        Ok(session)
    }

    async fn remove(&self, channel_key: &str) {
        let session = self.sessions.lock().await.remove(channel_key);
        if let Some(session) = session {
            let _ = session.child.lock().await.kill().await;
            let _ = tokio::fs::remove_dir_all(&session.dir).await;
        }
    }

    /// Kill sessions nobody has touched within [`IDLE_REAP`]. Called from the
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
}

/// `seg000042.ts` and nothing else — this becomes a filename under the
/// session dir, so it must never traverse.
fn is_valid_segment_name(name: &str) -> bool {
    name.strip_prefix("seg")
        .and_then(|rest| rest.strip_suffix(".ts"))
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_name_validation() {
        assert!(is_valid_segment_name("seg000001.ts"));
        assert!(is_valid_segment_name("seg9.ts"));
        assert!(!is_valid_segment_name("seg.ts"));
        assert!(!is_valid_segment_name("../etc/passwd"));
        assert!(!is_valid_segment_name("seg00/1.ts"));
        assert!(!is_valid_segment_name("seg1.ts.tmp"));
        assert!(!is_valid_segment_name("live.m3u8"));
    }
}
