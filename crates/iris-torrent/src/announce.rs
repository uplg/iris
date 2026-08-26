//! Explicit `stopped` announces on the way out of a swarm.
//!
//! librqbit never sends one on an HTTP tracker: as of 9.0.1
//! `TrackerRequestEvent::Stopped` exists but only the UDP path ever
//! constructs it, and the HTTP monitor sends `event=started` once then
//! `event=None` forever (`tracker_comms.rs::task_single_tracker_monitor_http`).
//! Deleting or pausing a torrent just drops that task, so the tracker keeps
//! counting us as an active peer until its own stale-peer pruning runs.
//!
//! On a tracker with a download-slot limit that is the difference between
//! "I removed it" and "I can't grab anything for the next hour" — seedpool
//! allows exactly one concurrent leech, and an add-then-remove at 0% left a
//! ghost leecher holding the only slot.
//!
//! The counters mirror what librqbit's own announces carry
//! (`session.rs::PeerRxTorrentInfo`: `downloaded = progress_bytes`,
//! `uploaded = uploaded_bytes`, `left = total - progress`), so the tracker
//! sees a continuous series and computes zero surprise deltas.

use std::time::Duration;

use url::Url;

/// Per-tracker budget. A stopped announce is best-effort — nothing
/// downstream may wait on a hung tracker, but it's worth a few seconds
/// because the alternative is a slot held hostage.
const ANNOUNCE_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct StoppedAnnounce {
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
    pub port: u16,
    pub uploaded: u64,
    pub downloaded: u64,
    pub left: u64,
}

impl StoppedAnnounce {
    /// BEP 3 announce query. `info_hash` / `peer_id` are raw 20-byte
    /// values, percent-encoded per byte — the same `urlencoding`
    /// treatment librqbit applies, so the tracker matches the peer row
    /// the `started` announce created.
    fn query(&self) -> String {
        use std::fmt::Write;
        let mut q = String::new();
        q.push_str("info_hash=");
        q.push_str(&urlencoding::encode_binary(&self.info_hash));
        q.push_str("&peer_id=");
        q.push_str(&urlencoding::encode_binary(&self.peer_id));
        q.push_str("&event=stopped");
        write!(q, "&port={}", self.port).ok();
        write!(q, "&uploaded={}", self.uploaded).ok();
        write!(q, "&downloaded={}", self.downloaded).ok();
        write!(q, "&left={}", self.left).ok();
        q.push_str("&compact=1&no_peer_id=0&numwant=0");
        q
    }

    fn url_for(&self, tracker: &Url) -> Url {
        let mut url = tracker.clone();
        let mut query = self.query();
        // Private-tracker announce URLs carry the passkey in the query
        // string; keep it by appending the original query, exactly like
        // librqbit's own announce path does.
        if let Some(existing) = tracker.query() {
            query.push('&');
            query.push_str(existing);
        }
        url.set_query(Some(&query));
        url
    }
}

/// Tell every tracker of a torrent we're leaving the swarm. Never fails the
/// caller: a tracker that refuses, hangs or 404s is logged and skipped.
pub(crate) async fn announce_stopped(
    http: &reqwest::Client,
    trackers: impl IntoIterator<Item = Url>,
    announce: &StoppedAnnounce,
) {
    let infohash = hex::encode(announce.info_hash);
    let calls = trackers.into_iter().map(|tracker| {
        let url = announce.url_for(&tracker);
        let infohash = &infohash;
        async move {
            let res = tokio::time::timeout(ANNOUNCE_TIMEOUT, http.get(url).send()).await;
            match res {
                Ok(Ok(r)) => tracing::debug!(
                    infohash = %infohash,
                    host = tracker.host_str().unwrap_or("?"),
                    status = %r.status(),
                    "announced stopped",
                ),
                Ok(Err(e)) => tracing::warn!(
                    infohash = %infohash,
                    host = tracker.host_str().unwrap_or("?"),
                    error = %e,
                    "stopped announce failed — the tracker may keep us listed as an active peer",
                ),
                Err(_) => tracing::warn!(
                    infohash = %infohash,
                    host = tracker.host_str().unwrap_or("?"),
                    "stopped announce timed out — the tracker may keep us listed as an active peer",
                ),
            }
        }
    });
    futures::future::join_all(calls).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> StoppedAnnounce {
        StoppedAnnounce {
            info_hash: [
                0x92, 0xcd, 0x5b, 0xe5, 0x63, 0x39, 0x6c, 0xf7, 0x7e, 0xc0, 0xb8, 0xf6, 0x21, 0x3a,
                0x6d, 0x4d, 0x87, 0x14, 0x31, 0xb2,
            ],
            peer_id: *b"-rQ9000-abcdefghijkl",
            port: 6881,
            uploaded: 12,
            downloaded: 34,
            left: 56,
        }
    }

    #[test]
    fn encodes_raw_bytes_and_the_event() {
        let q = sample().query();
        assert!(q.starts_with("info_hash=%92%CD%5B%E5c9l%F7~%C0%B8%F6%21%3AmM%87%141%B2"));
        assert!(q.contains("&peer_id=-rQ9000-abcdefghijkl"));
        assert!(q.contains("&event=stopped"));
        assert!(q.contains("&port=6881"));
        assert!(q.contains("&uploaded=12"));
        assert!(q.contains("&downloaded=34"));
        assert!(q.contains("&left=56"));
    }

    #[test]
    fn keeps_the_passkey_already_on_the_announce_url() {
        let tracker = Url::parse("https://tracker.example/announce?passkey=deadbeef").unwrap();
        let url = sample().url_for(&tracker);
        assert_eq!(url.path(), "/announce");
        let query = url.query().unwrap();
        assert!(query.contains("event=stopped"));
        assert!(query.ends_with("&passkey=deadbeef"));
    }
}
