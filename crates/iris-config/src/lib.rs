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
    pub providers_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbConfig {
    pub api_key: String,
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
