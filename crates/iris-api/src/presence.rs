//! In-memory live-playback presence registry.
//!
//! Every playback-progress heartbeat (`PUT .../progress`, ~7 s cadence on
//! web, whatever the TV emits) updates one entry per user here. The admin
//! "Now watching" view reads a TTL-filtered snapshot. This is also the
//! shared-state foundation for the upcoming watch-party feature (presence
//! is the substrate a future SSE sync channel broadcasts over).
//!
//! Not persisted: process-local, rebuilt from heartbeats after a restart.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::client_version::ClientKind;

/// How long after the last heartbeat a session is still "live". Web
/// heartbeats land every ~7 s; this tolerates ~6 missed beats. A paused
/// player stops emitting `timeupdate`, so paused sessions naturally age
/// out after this window — acceptable for "who's watching now".
pub const SESSION_TTL: Duration = Duration::from_secs(45);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Playing,
    Paused,
    /// Meant to play, waiting for data (buffering, seeking).
    Buffering,
}

impl PlaybackState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Playing => "playing",
            Self::Paused => "paused",
            Self::Buffering => "buffering",
        }
    }
}

/// One user's current playback. Keyed by user — a user watches one thing at
/// a time, so a new `(infohash, file_idx)` replaces the prior entry.
#[derive(Debug, Clone)]
pub struct LiveSession {
    pub user_id: Uuid,
    pub infohash: String,
    pub file_idx: i64,
    pub position_seconds: f64,
    pub duration_seconds: Option<f64>,
    pub state: PlaybackState,
    pub client: Option<ClientKind>,
    /// Semver of the client that sent the last heartbeat (the `version`
    /// half of `X-Iris-Client: kind/version`). `None` for legacy clients
    /// that don't stamp the header.
    pub client_version: Option<String>,
    /// The browser and system of a web client (« Firefox · macOS »), read
    /// from its User-Agent; `None` for the TV app or an unknown agent.
    pub browser: Option<String>,
    /// When this user started the *current* `(infohash, file_idx)`. Reset
    /// when they switch titles, preserved across heartbeats of the same one.
    pub started_at: DateTime<Utc>,
    /// Wall-clock of the last heartbeat (for display).
    pub last_seen_at: DateTime<Utc>,
    /// Monotonic deadline source for TTL pruning (immune to clock changes).
    last_seen: Instant,
}

/// The fields one heartbeat carries into the registry.
pub struct Heartbeat {
    pub user_id: Uuid,
    pub infohash: String,
    pub file_idx: i64,
    pub position_seconds: f64,
    pub duration_seconds: Option<f64>,
    pub state: PlaybackState,
    pub client: Option<ClientKind>,
    pub client_version: Option<String>,
    pub browser: Option<String>,
}

/// A browser and its system as people name them, from a User-Agent:
/// « Firefox · macOS », « Safari · iPadOS ». `None` when neither is known.
pub fn browser_of(agent: &str) -> Option<String> {
    // the order matters: Edge and Opera say Chrome too, Chrome says Safari
    let browser = [
        ("Edg/", "Edge"),
        ("OPR/", "Opera"),
        ("Firefox/", "Firefox"),
        ("FxiOS/", "Firefox"),
        ("CriOS/", "Chrome"),
        ("Chrome/", "Chrome"),
        ("Safari/", "Safari"),
    ]
    .into_iter()
    .find(|(token, _)| agent.contains(token))
    .map(|(_, name)| name);
    let system = [
        ("iPad", "iPadOS"),
        ("iPhone", "iOS"),
        ("Android", "Android"),
        ("Windows", "Windows"),
        ("Mac OS X", "macOS"),
        ("CrOS", "ChromeOS"),
        ("Linux", "Linux"),
    ]
    .into_iter()
    .find(|(token, _)| agent.contains(token))
    .map(|(_, name)| name);
    match (browser, system) {
        (Some(b), Some(s)) => Some(format!("{b} · {s}")),
        (Some(one), None) | (None, Some(one)) => Some(one.to_owned()),
        (None, None) => None,
    }
}

#[derive(Clone)]
pub struct Presence {
    inner: Arc<RwLock<HashMap<Uuid, LiveSession>>>,
}

impl Default for Presence {
    fn default() -> Self {
        Self::new()
    }
}

impl Presence {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Record a heartbeat. Preserves `started_at` while the user stays on the
    /// same `(infohash, file_idx)`; resets it when they switch titles.
    pub async fn touch(&self, hb: Heartbeat) {
        let now_utc = Utc::now();
        let now = Instant::now();
        let mut map = self.inner.write().await;
        match map.get_mut(&hb.user_id) {
            Some(s) if s.infohash == hb.infohash && s.file_idx == hb.file_idx => {
                s.position_seconds = hb.position_seconds;
                if hb.duration_seconds.is_some() {
                    s.duration_seconds = hb.duration_seconds;
                }
                s.state = hb.state;
                if hb.client.is_some() {
                    s.client = hb.client;
                }
                if hb.client_version.is_some() {
                    s.client_version = hb.client_version;
                }
                if hb.browser.is_some() {
                    s.browser = hb.browser;
                }
                s.last_seen_at = now_utc;
                s.last_seen = now;
            }
            _ => {
                map.insert(
                    hb.user_id,
                    LiveSession {
                        user_id: hb.user_id,
                        infohash: hb.infohash,
                        file_idx: hb.file_idx,
                        position_seconds: hb.position_seconds,
                        duration_seconds: hb.duration_seconds,
                        state: hb.state,
                        client: hb.client,
                        client_version: hb.client_version,
                        browser: hb.browser,
                        started_at: now_utc,
                        last_seen_at: now_utc,
                        last_seen: now,
                    },
                );
            }
        }
    }

    /// Drop a user's session (playback completed, explicit leave).
    pub async fn remove(&self, user_id: Uuid) {
        self.inner.write().await.remove(&user_id);
    }

    /// Live sessions (last heartbeat within [`SESSION_TTL`]), pruning the
    /// expired ones in place. Sorted most-recently-seen first.
    pub async fn snapshot(&self) -> Vec<LiveSession> {
        let now = Instant::now();
        let mut map = self.inner.write().await;
        map.retain(|_, s| is_live(s.last_seen, now, SESSION_TTL));
        let mut out: Vec<LiveSession> = map.values().cloned().collect();
        out.sort_by_key(|s| std::cmp::Reverse(s.last_seen_at));
        out
    }
}

/// Whether a session last seen at `last_seen` is still live at `now`.
/// `Instant::duration_since` saturates to zero for a future `last_seen`,
/// so this never panics on clock skew.
fn is_live(last_seen: Instant, now: Instant, ttl: Duration) -> bool {
    now.duration_since(last_seen) < ttl
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hb(user: Uuid, infohash: &str, idx: i64, pos: f64) -> Heartbeat {
        Heartbeat {
            user_id: user,
            infohash: infohash.to_owned(),
            file_idx: idx,
            position_seconds: pos,
            duration_seconds: Some(100.0),
            state: PlaybackState::Playing,
            client: Some(ClientKind::Web),
            client_version: Some("0.3.0".to_owned()),
            browser: None,
        }
    }

    #[tokio::test]
    async fn touch_creates_then_updates_position_preserving_started_at() {
        let p = Presence::new();
        let u = Uuid::new_v4();
        p.touch(hb(u, "ab", 0, 10.0)).await;
        let first = p.snapshot().await;
        assert_eq!(first.len(), 1);
        let started = first[0].started_at;
        assert!((first[0].position_seconds - 10.0).abs() < f64::EPSILON);

        // Same title: position advances, started_at unchanged.
        p.touch(hb(u, "ab", 0, 25.0)).await;
        let again = p.snapshot().await;
        assert_eq!(again.len(), 1);
        assert!((again[0].position_seconds - 25.0).abs() < f64::EPSILON);
        assert_eq!(
            again[0].started_at, started,
            "same title must keep started_at"
        );
    }

    #[tokio::test]
    async fn switching_title_resets_started_at() {
        let p = Presence::new();
        let u = Uuid::new_v4();
        p.touch(hb(u, "ab", 0, 10.0)).await;
        let started = p.snapshot().await[0].started_at;
        // A different file_idx is a new session for the same user.
        p.touch(hb(u, "ab", 1, 0.0)).await;
        let s = p.snapshot().await;
        assert_eq!(s.len(), 1, "one user => at most one live session");
        assert_eq!(s[0].file_idx, 1);
        assert!(s[0].started_at >= started);
    }

    #[tokio::test]
    async fn remove_drops_the_session() {
        let p = Presence::new();
        let u = Uuid::new_v4();
        p.touch(hb(u, "ab", 0, 10.0)).await;
        p.remove(u).await;
        assert!(p.snapshot().await.is_empty());
    }

    #[test]
    fn expired_sessions_are_not_live() {
        let now = Instant::now();
        let fresh = now.checked_sub(Duration::from_secs(5)).unwrap();
        let stale = now.checked_sub(Duration::from_mins(1)).unwrap();
        assert!(is_live(fresh, now, SESSION_TTL));
        assert!(!is_live(stale, now, SESSION_TTL));
    }

    #[test]
    fn a_browser_and_its_system_in_words() {
        let firefox =
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 15.6; rv:154.0) Gecko/20100101 Firefox/154.0";
        assert_eq!(browser_of(firefox).as_deref(), Some("Firefox · macOS"));
        let edge = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36 Edg/140.0";
        assert_eq!(browser_of(edge).as_deref(), Some("Edge · Windows"));
        let ipad = "Mozilla/5.0 (iPad; CPU OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1";
        assert_eq!(browser_of(ipad).as_deref(), Some("Safari · iPadOS"));
        let chrome = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36";
        assert_eq!(browser_of(chrome).as_deref(), Some("Chrome · Linux"));
        assert_eq!(browser_of("curl/8.0"), None);
    }

    #[tokio::test]
    async fn a_buffering_heartbeat_says_so() {
        let p = Presence::new();
        let u = Uuid::new_v4();
        p.touch(Heartbeat {
            state: PlaybackState::Buffering,
            browser: Some("Firefox · macOS".to_owned()),
            ..hb(u, "ab", 0, 10.0)
        })
        .await;
        let now = p.snapshot().await;
        assert_eq!(now[0].state.as_str(), "buffering");
        assert_eq!(now[0].browser.as_deref(), Some("Firefox · macOS"));
        // a later heartbeat that does not say its browser keeps the one known
        p.touch(hb(u, "ab", 0, 12.0)).await;
        let later = p.snapshot().await;
        assert_eq!(later[0].state.as_str(), "playing");
        assert_eq!(later[0].browser.as_deref(), Some("Firefox · macOS"));
    }
}
