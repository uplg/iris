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
}

impl PlaybackState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Playing => "playing",
            Self::Paused => "paused",
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
        assert_eq!(again[0].started_at, started, "same title must keep started_at");
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
        let stale = now.checked_sub(Duration::from_secs(60)).unwrap();
        assert!(is_live(fresh, now, SESSION_TTL));
        assert!(!is_live(stale, now, SESSION_TTL));
    }
}
