//! Engine — owning wrapper around `librqbit::Session`.
//!
//! All network/disk activity for torrents flows through here. The HTTP layer
//! holds an `Arc<Engine>` in app state and never touches `librqbit` types
//! directly.

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, Session, SessionOptions,
    SessionPersistenceConfig, TorrentStatsState,
};
use serde::Serialize;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncSeek};

/// `librqbit` re-exports `ManagedTorrentHandle` as a private type alias inside
/// `torrent_state`; keep the local alias explicit so callers don't need to
/// know the inner shape.
type Handle = Arc<ManagedTorrent>;

/// Trait alias used to box librqbit's private `FileStream` (which isn't
/// re-exported from the crate root) for the HTTP-Range layer.
pub trait Streamable: AsyncRead + AsyncSeek + Unpin + Send {}
impl<T: AsyncRead + AsyncSeek + Unpin + Send + ?Sized> Streamable for T {}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("torrent not found")]
    NotFound,
    #[error("file index out of range")]
    FileOutOfRange,
    #[error("librqbit: {0}")]
    Librqbit(#[from] anyhow::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TorrentState {
    Initializing,
    Live,
    Paused,
    Error,
}

impl TorrentState {
    fn from_librqbit(s: TorrentStatsState) -> Self {
        match s {
            TorrentStatsState::Initializing => Self::Initializing,
            TorrentStatsState::Live => Self::Live,
            TorrentStatsState::Paused => Self::Paused,
            TorrentStatsState::Error => Self::Error,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub index: usize,
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TorrentSnapshot {
    pub infohash: String,
    pub name: Option<String>,
    pub total_size_bytes: u64,
    pub state: TorrentState,
    pub progress_bytes: u64,
    pub progress_pct: f64,
    pub download_speed_bps: u64,
    pub upload_speed_bps: u64,
    pub uploaded_bytes: u64,
    pub peers: u32,
    pub files: Vec<FileEntry>,
    pub error: Option<String>,
    pub finished: bool,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IngestResult {
    pub already_managed: bool,
    pub snapshot: TorrentSnapshot,
}

pub struct Engine {
    session: Arc<Session>,
    download_dir: PathBuf,
}

impl Engine {
    pub async fn new(
        download_dir: PathBuf,
        persistence_dir: PathBuf,
        listen_port: u16,
    ) -> anyhow::Result<Arc<Self>> {
        std::fs::create_dir_all(&download_dir)?;
        std::fs::create_dir_all(&persistence_dir)?;
        let opts = SessionOptions {
            fastresume: true,
            persistence: Some(SessionPersistenceConfig::Json {
                folder: Some(persistence_dir),
            }),
            // Pin the BitTorrent listen port so docker can forward it and
            // peers can reach us. Inbound connections roughly double download
            // speed on private trackers and let us actually upload (= keep
            // ratio sane).
            listen_port_range: Some(listen_port..(listen_port + 1)),
            // Try UPnP on the local router; harmless on a server with a
            // public IP (the router-side step just no-ops).
            enable_upnp_port_forwarding: true,
            ..Default::default()
        };
        let session = Session::new_with_opts(download_dir.clone(), opts).await?;
        Ok(Arc::new(Self {
            session,
            download_dir,
        }))
    }

    pub fn download_dir(&self) -> &std::path::Path {
        &self.download_dir
    }

    pub async fn add_from_bytes(&self, bytes: Vec<u8>) -> Result<IngestResult, EngineError> {
        let res = self
            .session
            .add_torrent(
                AddTorrent::TorrentFileBytes(Bytes::from(bytes)),
                Some(AddTorrentOptions {
                    overwrite: true,
                    ..Default::default()
                }),
            )
            .await?;
        Self::wrap(res)
    }

    pub async fn add_from_magnet(&self, magnet: &str) -> Result<IngestResult, EngineError> {
        let res = self
            .session
            .add_torrent(
                AddTorrent::Url(magnet.into()),
                Some(AddTorrentOptions {
                    overwrite: true,
                    ..Default::default()
                }),
            )
            .await?;
        Self::wrap(res)
    }

    fn wrap(res: AddTorrentResponse) -> Result<IngestResult, EngineError> {
        match res {
            AddTorrentResponse::Added(_, h) => Ok(IngestResult {
                already_managed: false,
                snapshot: snapshot_of(&h),
            }),
            AddTorrentResponse::AlreadyManaged(_, h) => Ok(IngestResult {
                already_managed: true,
                snapshot: snapshot_of(&h),
            }),
            AddTorrentResponse::ListOnly(_) => Err(EngineError::Librqbit(anyhow::anyhow!(
                "unexpected list-only response"
            ))),
        }
    }

    pub fn list(&self) -> Vec<TorrentSnapshot> {
        self.session.with_torrents(|iter| {
            let mut out = Vec::new();
            for (_, h) in iter {
                out.push(snapshot_of(h));
            }
            out
        })
    }

    pub fn get_by_infohash(&self, infohash: &str) -> Option<TorrentSnapshot> {
        self.handle_by_infohash(infohash)
            .ok()
            .map(|h| snapshot_of(&h))
    }

    fn handle_by_infohash(&self, infohash: &str) -> Result<Handle, EngineError> {
        let needle = infohash.to_ascii_lowercase();
        let handle = self.session.with_torrents(|iter| {
            for (_, h) in iter {
                if hex::encode(h.info_hash().0) == needle {
                    return Some(h.clone());
                }
            }
            None
        });
        handle.ok_or(EngineError::NotFound)
    }

    pub async fn delete_by_infohash(
        &self,
        infohash: &str,
        delete_files: bool,
    ) -> Result<(), EngineError> {
        let handle = self.handle_by_infohash(infohash)?;
        self.session.delete(handle.id().into(), delete_files).await?;
        Ok(())
    }

    /// Resolve the absolute on-disk path librqbit writes to for a given
    /// torrent file. Used by the remux pipeline to feed ffmpeg an actual file
    /// instead of replaying through the streaming layer.
    pub fn file_path(
        &self,
        infohash: &str,
        file_idx: usize,
    ) -> Result<PathBuf, EngineError> {
        let handle = self.handle_by_infohash(infohash)?;
        let rel = handle
            .with_metadata(|m| {
                m.file_infos
                    .get(file_idx)
                    .map(|fi| fi.relative_filename.clone())
                    .ok_or_else(|| anyhow::anyhow!("file index out of range"))
            })
            .map_err(EngineError::Librqbit)??;
        let direct = self.download_dir.join(&rel);
        if direct.exists() {
            return Ok(direct);
        }
        // Multi-file torrents: librqbit nests files inside a folder named
        // after the torrent's `info.name`.
        if let Some(name) = handle.name() {
            let nested = self.download_dir.join(name).join(&rel);
            if nested.exists() {
                return Ok(nested);
            }
            return Ok(nested);
        }
        Ok(direct)
    }

    /// Open a streaming reader for one file. The returned reader implements
    /// `AsyncRead + AsyncSeek` and triggers sequential piece priority in
    /// librqbit, which is what makes "click-to-play" feasible.
    pub fn open_stream(
        &self,
        infohash: &str,
        file_idx: usize,
    ) -> Result<StreamHandle, EngineError> {
        let handle = self.handle_by_infohash(infohash)?;
        let file_size = handle
            .with_metadata(|m| {
                m.file_infos
                    .get(file_idx)
                    .map(|fi| fi.len)
                    .ok_or_else(|| anyhow::anyhow!("file index out of range"))
            })
            .map_err(EngineError::Librqbit)??;
        let stream = handle.stream(file_idx)?;
        Ok(StreamHandle {
            inner: Box::pin(stream),
            file_size,
        })
    }
}

fn snapshot_of(handle: &Handle) -> TorrentSnapshot {
    let stats = handle.stats();
    let files = handle
        .with_metadata(|m| {
            m.file_infos
                .iter()
                .enumerate()
                .map(|(idx, fi)| FileEntry {
                    index: idx,
                    path: fi.relative_filename.to_string_lossy().to_string(),
                    size_bytes: fi.len,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let total = if stats.total_bytes > 0 {
        stats.total_bytes
    } else {
        files.iter().map(|f| f.size_bytes).sum()
    };
    let progress_pct = if total > 0 {
        progress_pct(stats.progress_bytes, total)
    } else {
        0.0
    };
    let (down_bps, up_bps, peers) = match stats.live.as_ref() {
        Some(l) => (
            mbps_to_bps(l.download_speed.mbps),
            mbps_to_bps(l.upload_speed.mbps),
            u32::try_from(l.snapshot.peer_stats.live).unwrap_or(u32::MAX),
        ),
        None => (0, 0, 0),
    };
    TorrentSnapshot {
        infohash: hex::encode(handle.info_hash().0),
        name: handle.name(),
        total_size_bytes: total,
        state: TorrentState::from_librqbit(stats.state),
        progress_bytes: stats.progress_bytes,
        progress_pct,
        download_speed_bps: down_bps,
        upload_speed_bps: up_bps,
        uploaded_bytes: stats.uploaded_bytes,
        peers,
        files,
        error: stats.error,
        finished: stats.finished,
        fetched_at: Utc::now(),
    }
}

#[allow(clippy::cast_precision_loss)]
fn progress_pct(progress: u64, total: u64) -> f64 {
    // u64 → f64 only loses precision past 2^53 bytes (~9 PB). No real
    // torrent gets near that, and the result is rendered as a UI percentage
    // so a few bytes of imprecision are invisible.
    progress as f64 / total as f64 * 100.0
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn mbps_to_bps(mbps: f64) -> u64 {
    // Speed is reported in megabits per second; convert to bytes per second.
    // Bounded above by realistic peer throughput (single-digit GB/s), so the
    // f64 → u64 cast is safe; max(0.0) covers transient negative readings
    // from librqbit's smoothing.
    (mbps * 125_000.0).round().max(0.0) as u64
}

pub struct StreamHandle {
    inner: Pin<Box<dyn Streamable>>,
    file_size: u64,
}

impl StreamHandle {
    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    pub fn into_reader(self) -> Pin<Box<dyn Streamable>> {
        self.inner
    }
}
