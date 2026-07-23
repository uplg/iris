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
    pub live_tv: LiveTvConfig,
    #[serde(default)]
    pub providers_file: Option<PathBuf>,
}

/// Live TV: per-country IPTV playlists (iptv-org) exposed as channel lists
/// and played through the backend HLS proxy (upstreams are plain-http /
/// CORS-less and some require a browser User-Agent, so clients can never hit
/// them direct). Countries are loaded lazily on first access and refreshed
/// in the background.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveTvConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Country pre-selected by clients (ISO 3166-1 alpha-2, lowercase).
    #[serde(default = "default_livetv_country")]
    pub default_country: String,
    /// Per-country playlist URL; `{code}` is replaced by the lowercase
    /// country code.
    #[serde(default = "default_livetv_playlist_template")]
    pub playlist_url_template: String,
    /// Country catalogue (code / name / flag) shown in the country picker.
    #[serde(default = "default_livetv_countries_url")]
    pub countries_url: String,
    /// iptv-org's full stream database. The per-country playlists only embed
    /// ONE feed per channel; this database lists them all, and every extra
    /// feed becomes a fallback source the proxy can rotate to.
    #[serde(default = "default_livetv_streams_url")]
    pub streams_url: String,
    /// iptv-org's channel metadata database (names, countries, logos) —
    /// powers the cross-country channel search.
    #[serde(default = "default_livetv_channels_url")]
    pub channels_url: String,
    /// Extra fallback playlists per country code. Entries matching a channel
    /// already found in the iptv-org playlist (by tvg-id / name) merge in as
    /// additional fallback sources — cross-provider redundancy, not dupes.
    /// Defaults ship curated FR / US / IE lists (official-CDN + FAST-provider
    /// heavy); the source-tier election prefers those over iptv-org's feeds.
    #[serde(default = "default_livetv_extra_playlists")]
    pub extra_playlists: HashMap<String, Vec<String>>,
    /// Gzipped XMLTV programme guide per country code, for the now/next
    /// overlay. Countries without an entry simply have no guide.
    #[serde(default = "default_livetv_epg_urls")]
    pub epg_urls: HashMap<String, String>,
    #[serde(default = "default_livetv_playlist_refresh_hours")]
    pub playlist_refresh_hours: u64,
    #[serde(default = "default_livetv_epg_refresh_hours")]
    pub epg_refresh_hours: u64,
    /// tvg-id base (e.g. "TF1") → TNT channel number; merged over the
    /// built-in table so a renamed tvg-id can be re-pinned without a
    /// release. Only meaningful for `fr`.
    #[serde(default)]
    pub tnt_overrides: HashMap<String, u16>,
    /// channel id (slug) → XMLTV channel id, for guide ids that don't match
    /// the playlist's tvg-id.
    #[serde(default)]
    pub epg_id_overrides: HashMap<String, String>,
    /// Pull channels from Vavoo's aggregator (clear-HLS restreams of European
    /// TNT, resolved on demand) as extra Community-tier sources. This is the
    /// only live source still carrying the M6 group after its restreams were
    /// pulled everywhere else.
    #[serde(default = "default_true")]
    pub vavoo_enabled: bool,
    /// Country code → Vavoo "group" names to fold into that country's list.
    /// A country absent here simply gets no Vavoo channels.
    #[serde(default = "default_livetv_vavoo_countries")]
    pub vavoo_countries: HashMap<String, Vec<String>>,
    /// The household's own DVB-T tuner (tunerd on the AIR 7310T box).
    /// Channels listed here are served from the antenna with ABSOLUTE
    /// priority (`SourceTier::Tuner`); every internet source stays in place
    /// as automatic fallback when the box is unreachable.
    #[serde(default)]
    pub tuner: TunerConfig,
}

/// `[live_tv.tuner]` — the tunerd network-tuner appliance. That's the whole
/// config: the box knows its own channel grid and serves it at
/// `{base_url}/channels` (names, frequencies, hardware-filter PIDs from its
/// mux survey); Iris discovers it on every playlist load.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TunerConfig {
    #[serde(default)]
    pub enabled: bool,
    /// tunerd base URL, reachable from the Iris host (tailnet address),
    /// e.g. `"http://100.101.102.103:8554"`.
    #[serde(default)]
    pub base_url: String,
    /// Channel ids (Iris slugs, e.g. `"m6"`) whose tuner sessions are
    /// hydrated at boot and kept warm forever. A viewer joining a warm
    /// session starts in seconds with a full buffer; a cold one stutters
    /// through its first minute — so the server pays that cost once, with
    /// nobody watching. Keep the list within TWO muxes: tunerd has two
    /// adapters, and a third mux evicts a warm one.
    #[serde(default)]
    pub prewarm: Vec<String>,
}

fn default_livetv_country() -> String {
    "fr".to_string()
}
fn default_livetv_playlist_template() -> String {
    "https://iptv-org.github.io/iptv/countries/{code}.m3u".to_string()
}
fn default_livetv_countries_url() -> String {
    "https://iptv-org.github.io/api/countries.json".to_string()
}
fn default_livetv_streams_url() -> String {
    "https://iptv-org.github.io/api/streams.json".to_string()
}
fn default_livetv_channels_url() -> String {
    "https://iptv-org.github.io/api/channels.json".to_string()
}
fn default_livetv_extra_playlists() -> HashMap<String, Vec<String>> {
    // Curated per-country lists merged (by tvg-id / name) as extra fallback
    // sources on top of iptv-org's base list + stream database. Their feeds
    // lean on official CDNs / FAST providers, which the source-tier election
    // ranks above iptv-org's community feeds. Only for countries actually
    // used here — adding a country means vetting its list is alive first.
    let ftv = |slug: &str| {
        format!(
            "https://raw.githubusercontent.com/Free-TV/IPTV/master/playlists/playlist_{slug}.m3u8"
        )
    };
    HashMap::from([
        (
            // ParaTV is CI-refreshed several times a day (full TNT via
            // raw.github indirection playlists whose content is re-signed
            // continuously — the URLs themselves are stable). schumijo's
            // top-level fr.m3u8 is NOT refreshed (its inline tokens rot) but
            // its static entries (RMC/FAST cloudfront feeds) stay valuable.
            // Free-TV is a small curated "officially free" set. Many entries
            // only answer from residential IPs — the variant-validating
            // election skips those cleanly on a datacenter deployment.
            "fr".to_string(),
            vec![
                "https://raw.githubusercontent.com/Paradise-91/ParaTV/main/playlists/paratv/group/france/france.m3u".to_string(),
                "https://raw.githubusercontent.com/schumijo/iptv/main/fr.m3u8".to_string(),
                ftv("france"),
            ],
        ),
        (
            // US free TV is carried by licensed FAST providers (Pluto, Tubi,
            // Amagi, Publica…) — Free-TV's US list surfaces the stable ones.
            "us".to_string(),
            vec![ftv("usa")],
        ),
        // Irish free-to-air is thin; Free-TV's curated IE list is the best
        // stable supplement to iptv-org's aggregated feeds.
        ("ie".to_string(), vec![ftv("ireland")]),
    ])
}
fn default_livetv_vavoo_countries() -> HashMap<String, Vec<String>> {
    // Vavoo's "group" is a loose region label; map each to the ISO country
    // whose channel list should absorb it. "France Sport" also feeds `fr`.
    // Arabia / Balkans have no single ISO country — attached to a
    // representative code so they stay reachable.
    let one = |g: &str| vec![g.to_string()];
    HashMap::from([
        (
            "fr".to_string(),
            vec!["France".to_string(), "France Sport".to_string()],
        ),
        ("de".to_string(), one("Germany")),
        ("it".to_string(), one("Italy")),
        ("es".to_string(), one("Spain")),
        ("gb".to_string(), one("United Kingdom")),
        ("nl".to_string(), one("Netherlands")),
        ("pl".to_string(), one("Poland")),
        ("pt".to_string(), one("Portugal")),
        ("ru".to_string(), one("Russia")),
        ("tr".to_string(), one("Turkey")),
        ("ro".to_string(), one("Romania")),
        ("bg".to_string(), one("Bulgaria")),
        ("hr".to_string(), one("Croatia")),
        ("al".to_string(), one("Albania")),
        ("sa".to_string(), one("Arabia")),
        ("rs".to_string(), one("Balkans")),
    ])
}
fn default_livetv_epg_urls() -> HashMap<String, String> {
    HashMap::from([(
        "fr".to_string(),
        "https://xmltvfr.fr/xmltv/xmltv_tnt.xml.gz".to_string(),
    )])
}
fn default_livetv_playlist_refresh_hours() -> u64 {
    // iptv-org regenerates its playlists + stream database several times a
    // day (dead feeds pruned, alternates added) — track it reasonably close.
    6
}
fn default_livetv_epg_refresh_hours() -> u64 {
    6
}

impl Default for LiveTvConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_country: default_livetv_country(),
            playlist_url_template: default_livetv_playlist_template(),
            countries_url: default_livetv_countries_url(),
            streams_url: default_livetv_streams_url(),
            channels_url: default_livetv_channels_url(),
            extra_playlists: default_livetv_extra_playlists(),
            epg_urls: default_livetv_epg_urls(),
            tuner: TunerConfig::default(),
            playlist_refresh_hours: default_livetv_playlist_refresh_hours(),
            epg_refresh_hours: default_livetv_epg_refresh_hours(),
            tnt_overrides: HashMap::new(),
            epg_id_overrides: HashMap::new(),
            vavoo_enabled: true,
            vavoo_countries: default_livetv_vavoo_countries(),
        }
    }
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
impl Default for RecoConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            model: default_reco_model(),
            centroids: default_reco_centroids(),
            embed_batch: default_reco_embed_batch(),
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
    /// `Secure` attribute on session cookies (cookie withheld over plain HTTP).
    /// `None` (default) derives it from `server.public_url`: behind TLS (https)
    /// → secure, plain-http dev (localhost) → not, so dev login keeps working.
    /// Set explicitly to force either way. See [`AppConfig::cookie_secure`].
    #[serde(default)]
    pub cookie_secure: Option<bool>,
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

    /// Whether session cookies get the `Secure` attribute. Explicit
    /// `auth.cookie_secure` wins; otherwise inferred from the public URL scheme
    /// (`https://` → secure). Keeps dev (`http://localhost`) working with no
    /// config, and turns Secure on automatically once deployed behind TLS.
    pub fn cookie_secure(&self) -> bool {
        self.auth
            .cookie_secure
            .unwrap_or_else(|| self.server.public_url.trim_start().starts_with("https://"))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A config with no `[live_tv]` section must still ship the curated FR
    /// fallback playlists — that's what gives every French channel real
    /// cross-provider redundancy out of the box. Locks the default against an
    /// accidental empty-map regression.
    #[test]
    fn livetv_ships_curated_extra_playlists_by_default() {
        let cfg = LiveTvConfig::default();
        let fr = cfg.extra_playlists.get("fr").expect("fr defaults present");
        assert_eq!(fr.len(), 3, "ParaTV + schumijo + Free-TV");
        assert!(fr.iter().any(|u| u.contains("ParaTV")));
        assert!(fr.iter().any(|u| u.contains("schumijo")));
        assert!(cfg.extra_playlists.contains_key("us"));
        assert!(cfg.extra_playlists.contains_key("ie"));
        assert!(cfg.extra_playlists["us"].iter().any(|u| u.contains("usa")));
        assert!(
            cfg.extra_playlists["ie"]
                .iter()
                .any(|u| u.contains("ireland"))
        );
        // Deserializing an empty document (no [live_tv]) yields the same —
        // serde field defaults fire, so a bare prod config.toml is covered.
        let bare: LiveTvConfig = toml::from_str("").unwrap();
        assert_eq!(bare.extra_playlists, cfg.extra_playlists);
    }
}
