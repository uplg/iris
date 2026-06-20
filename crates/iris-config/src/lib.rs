//! Configuration loader for Iris.
//!
//! Loads `config.toml` (server / storage / auth) and `providers.toml`
//! (one entry per tracker provider). Environment variables can override
//! values via `IRIS_*` (double-underscore = nesting, e.g. `IRIS_SERVER__BIND`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use figment::Figment;
use figment::providers::{Env, Format, Toml};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config file not found: {0}")]
    NotFound(PathBuf),
    #[error("failed to parse config: {0}")]
    Parse(#[from] Box<figment::Error>),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub auth: AuthConfig,
    #[serde(default)]
    pub tmdb: Option<TmdbConfig>,
    #[serde(default)]
    pub discovery: DiscoveryConfig,
    #[serde(default)]
    pub transcode: TranscodeConfig,
    #[serde(default)]
    pub reco: RecoConfig,
    #[serde(default)]
    pub providers_file: Option<PathBuf>,
}

/// Tuning for the content-first recommendation engine (see `RECOSYS.md`). The
/// model embeds each catalogue item once at ingest; the request path only ranks
/// over the cached vectors, so none of this touches the hot path's memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoConfig {
    /// Master switch. When false the engine falls back to the legacy linear
    /// `fresh_score` shelves.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// `model2vec` model id (Hugging Face repo or local path). The English,
    /// retrieval-tuned `potion-retrieval-32M` won the rig sweep — smaller AND
    /// more accurate than the multilingual model — so embedding text is
    /// normalized to English. Swap to `potion-base-8M` for an ~8 MB table.
    #[serde(default = "default_reco_model")]
    pub model: String,
    /// Taste centroids per user (weighted k-means). 3 was the empirical optimum
    /// on the prod data (1 blurs a household's distinct tastes, 5 over-segments).
    #[serde(default = "default_reco_centroids")]
    pub centroids: usize,
    /// Max items embedded per ingest slice — bounds the background pass so a big
    /// backfill never monopolises the box.
    #[serde(default = "default_reco_embed_batch")]
    pub embed_batch: i64,
    /// Max recorded release size (GiB) a **movie** may have to be proposed by the
    /// reco / mood shelves. The catalogue records one best release per title via
    /// `recommended_cmp` (smallest-sane, seeders garde-fou), which can land on a
    /// 4K REMUX (75 GB) when smaller encodes are low-seeded. Above this the row is
    /// not proposed by default; the title can still surface via the discover path
    /// (re-searched for a saner release at grab). `0` disables the cap.
    #[serde(default = "default_max_movie_gib")]
    pub max_movie_gib: i64,
    /// Same cap for **tv** — higher, since a legit season pack is large.
    #[serde(default = "default_max_tv_gib")]
    pub max_tv_gib: i64,
}

fn default_reco_model() -> String {
    "minishlab/potion-retrieval-32M".to_string()
}
fn default_reco_centroids() -> usize {
    3
}
fn default_reco_embed_batch() -> i64 {
    512
}
fn default_max_movie_gib() -> i64 {
    40
}
fn default_max_tv_gib() -> i64 {
    100
}

impl RecoConfig {
    /// Size ceiling in bytes for a kind (`<= 0` ⇒ no cap). `kind` is the
    /// catalogue's `"movie"` / `"tv"` tag.
    #[must_use]
    pub fn max_bytes_for_kind(&self, kind: &str) -> Option<i64> {
        let gib = if kind == "tv" {
            self.max_tv_gib
        } else {
            self.max_movie_gib
        };
        (gib > 0).then(|| gib.saturating_mul(1_073_741_824))
    }
}

impl Default for RecoConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            model: default_reco_model(),
            centroids: default_reco_centroids(),
            embed_batch: default_reco_embed_batch(),
            max_movie_gib: default_max_movie_gib(),
            max_tv_gib: default_max_tv_gib(),
        }
    }
}

/// Server-side encode settings for the "catch-up" transcode path — used when
/// a client only software-decodes the source video codec (e.g. AV1 on a TV
/// box with no AV1 silicon) and the content is heavy (10-bit). The server
/// re-encodes to a codec the client hardware-decodes, capped at 1080p. On a
/// CPU-only server the preset MUST stay ahead of real-time playback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscodeConfig {
    /// Target codec: `"hevc"` (default — hardware-decoded by most TV chips,
    /// keeps 10-bit, ~2× smaller) or `"h264"` (8-bit, encodes much faster).
    #[serde(default = "default_transcode_codec")]
    pub codec: String,
    /// `libx264` / `libx265` `-preset`. Default `"superfast"`.
    #[serde(default = "default_transcode_preset")]
    pub preset: String,
    /// `-crf` (0..=51). Lower = better quality / larger. Default `26`.
    #[serde(default = "default_transcode_crf")]
    pub crf: u8,
    /// Keep a 10-bit pipeline (HEVC only). `false` (default) = 8-bit, which
    /// encodes faster + smaller and is invisible for SDR sources; `true`
    /// preserves the source's 10-bit precision.
    #[serde(default)]
    pub ten_bit: bool,
}

impl Default for TranscodeConfig {
    fn default() -> Self {
        Self {
            codec: default_transcode_codec(),
            preset: default_transcode_preset(),
            crf: default_transcode_crf(),
            ten_bit: false,
        }
    }
}

fn default_transcode_codec() -> String {
    // H.264 by default: `libx264` encodes several times faster than `libx265`,
    // which a CPU-only server needs to stay AHEAD of real-time playback (the
    // transcode streams into HLS as it encodes). HEVC is ~2× smaller on disk
    // but only viable when the server has the headroom — flip `codec = "hevc"`
    // then.
    "h264".to_string()
}
fn default_transcode_preset() -> String {
    "veryfast".to_string()
}
fn default_transcode_crf() -> u8 {
    26
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbConfig {
    pub api_key: String,
}

/// Tuning for the discovery "rolling window" freshness scheduler. The
/// scheduler polls each provider's latest-releases feed one slice at a time
/// (provider × kind) so tracker load stays spread out, keeps a sliding window
/// of recent releases correlated to TMDB, and GCs anything past retention.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    /// How far back (in weeks) a release may be and still enter the window.
    #[serde(default = "default_poll_window_weeks")]
    pub poll_window_weeks: i64,
    /// How long (in weeks) a windowed release is kept before the GC slides it
    /// out. Should be ≤ `poll_window_weeks`.
    #[serde(default = "default_retain_weeks")]
    pub retain_weeks: i64,
    /// Minutes between slices. One (provider × kind) slice runs per tick, so a
    /// full refresh cycle takes `slice_interval_minutes × providers × 2`.
    #[serde(default = "default_slice_interval_minutes")]
    pub slice_interval_minutes: u64,
    /// Max age (in years) of a MOVIE's content to enter the discovery window.
    /// The window is about recent *releases*: a 1972 film freshly re-uploaded
    /// is a fresh upload but not a fresh release, so it's kept out of the
    /// discovery shelves (it can still surface via recommendations / search).
    /// TV is exempt — a long-running series airing a new episode is legit
    /// regardless of its first-air year.
    #[serde(default = "default_max_content_age_years")]
    pub max_content_age_years: i64,
}

fn default_poll_window_weeks() -> i64 {
    8
}
fn default_retain_weeks() -> i64 {
    4
}
fn default_slice_interval_minutes() -> u64 {
    10
}
fn default_max_content_age_years() -> i64 {
    10
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            poll_window_weeks: default_poll_window_weeks(),
            retain_weeks: default_retain_weeks(),
            slice_interval_minutes: default_slice_interval_minutes(),
            max_content_age_years: default_max_content_age_years(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_public_url")]
    pub public_url: String,
    #[serde(default)]
    pub web_dist: Option<PathBuf>,
}

fn default_bind() -> String {
    "0.0.0.0:8080".into()
}
fn default_public_url() -> String {
    "http://localhost:8080".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub data_dir: PathBuf,
    pub download_dir: PathBuf,
    #[serde(default = "default_max_storage")]
    pub max_storage_gb: u64,
    #[serde(default = "default_cleanup_threshold")]
    pub cleanup_threshold_pct: u8,
    #[serde(default = "default_cleanup_target")]
    pub cleanup_target_pct: u8,
    /// Pinned `BitTorrent` listen port (TCP + UDP). Forward this in your
    /// firewall / docker for inbound peer connections.
    #[serde(default = "default_torrent_port")]
    pub torrent_port: u16,
}

fn default_max_storage() -> u64 {
    500
}
fn default_cleanup_threshold() -> u8 {
    90
}
fn default_cleanup_target() -> u8 {
    75
}
fn default_torrent_port() -> u16 {
    45100
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub jwt_secret: String,
    #[serde(default = "default_access_ttl")]
    pub access_ttl_secs: i64,
    #[serde(default = "default_refresh_ttl")]
    pub refresh_ttl_secs: i64,
    /// Refresh-token lifetime for device-paired sessions (Android TV).
    /// Much longer than the browser default: a TV is a trusted appliance on
    /// the household LAN, the token is revocable from the account UI, and a
    /// living-room device that 401s with a useless "Retry" is a terrible UX.
    /// The window is SLIDING — every refresh re-issues the full TTL — so a
    /// TV in regular use never expires; only one left off longer than this
    /// whole window needs re-pairing.
    #[serde(default = "default_device_refresh_ttl")]
    pub device_refresh_ttl_secs: i64,
    #[serde(default = "default_invite_ttl")]
    pub invitation_ttl_secs: i64,
    #[serde(default)]
    pub bootstrap_admin: Option<BootstrapAdmin>,
}

fn default_access_ttl() -> i64 {
    // Streaming sessions routinely exceed the old 15 min default — the
    // player would suddenly 401 mid-movie. 1 hour is a sane compromise:
    // long enough for a typical viewing session, short enough that a
    // stolen cookie has limited reach.
    3600 // 1 hour
}
fn default_refresh_ttl() -> i64 {
    7 * 24 * 3600
}
fn default_device_refresh_ttl() -> i64 {
    365 * 24 * 3600 // 1 year, sliding
}
fn default_invite_ttl() -> i64 {
    7 * 24 * 3600
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapAdmin {
    pub email: String,
    pub password: String,
}

/// Raw provider config: `kind` selects the implementation factory,
/// remaining fields are forwarded to it as a free-form map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderEntry {
    pub id: String,
    pub kind: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(flatten)]
    pub fields: HashMap<String, toml::Value>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProvidersConfig {
    #[serde(default, rename = "providers")]
    pub providers: Vec<ProviderEntry>,
}

impl AppConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(ConfigError::NotFound(path.to_path_buf()));
        }
        let cfg: Self = Figment::new()
            .merge(Toml::file(path))
            .merge(Env::prefixed("IRIS_").split("__"))
            .extract()
            .map_err(Box::new)?;
        Ok(cfg)
    }

    pub fn load_providers(&self, fallback: Option<&Path>) -> Result<ProvidersConfig, ConfigError> {
        let path = self
            .providers_file
            .clone()
            .or_else(|| fallback.map(Path::to_path_buf));
        let Some(path) = path else {
            return Ok(ProvidersConfig::default());
        };
        if !path.exists() {
            return Ok(ProvidersConfig::default());
        }
        let raw = std::fs::read_to_string(&path)?;
        let parsed: ProvidersConfig = toml::from_str(&raw)
            .map_err(|e| ConfigError::Parse(Box::new(figment::Error::from(e.to_string()))))?;
        Ok(parsed)
    }
}
