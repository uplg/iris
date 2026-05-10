//! Provider-agnostic search domain types.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default)]
    pub page: Option<u32>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub sort_by: Option<SortField>,
    #[serde(default)]
    pub order: Option<SortOrder>,
    /// Filter results to a specific media kind (movies or TV). Provider
    /// implementations translate this to their native taxonomy. `None`
    /// means "everything".
    #[serde(default)]
    pub kind: Option<MediaKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Movie,
    Tv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortField {
    Title,
    Size,
    Seeders,
    Leechers,
    Uploaded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderPage {
    pub results: Vec<SearchResult>,
    pub current_page: u32,
    pub limit: u32,
    pub total_count: Option<u64>,
    pub total_pages: Option<u32>,
}

#[derive(Debug, Clone, Default, Copy, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub returns_magnet: bool,
    pub returns_torrent_file: bool,
    pub returns_infohash: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub provider_id: String,
    pub external_id: String,
    pub title: String,
    pub year: Option<u16>,
    pub size_bytes: Option<u64>,
    pub seeders: Option<u32>,
    pub leechers: Option<u32>,
    pub infohash: Option<String>,
    pub magnet: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub freeleech: bool,
    #[serde(default)]
    pub uploader: Option<String>,
    #[serde(default)]
    pub uploaded_at: Option<chrono::DateTime<chrono::Utc>>,
    /// TMDB id (movie or TV), captured from providers that index it. Powers
    /// poster + metadata enrichment via `/api/metadata/tmdb/:id`.
    #[serde(default)]
    pub tmdb_id: Option<u64>,
    /// Coarse classification (movie / tv / unknown), derived from the
    /// provider's category taxonomy.
    #[serde(default)]
    pub kind: Option<MediaKind>,
    /// Pre-resolved poster URL when the indexer ships one (torr9
    /// includes it on featured items). Trusted because the indexer
    /// curates these editorially — much higher confidence than a
    /// TMDB-id round-trip.
    #[serde(default)]
    pub poster_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TorrentSource {
    Magnet(String),
    TorrentFile(Vec<u8>),
}

// ===========================================================================
// Torrent details (provider-specific "look at this torrent before grabbing")
// ===========================================================================

/// Rich detail view for a single torrent — the kind of payload a tracker's
/// torrent-detail page would normally show. Powers the search-result
/// preview dialog. Optional everywhere because shapes vary wildly across
/// indexers; the typed fields are the lowest common denominator we feel
/// comfortable surfacing in a unified UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentDetails {
    pub provider_id: String,
    pub external_id: String,
    pub title: String,
    /// BBCode-formatted description from the indexer (rendered in-app).
    pub description: Option<String>,
    /// Raw `MediaInfo` text dump. Web exposes this in a collapsible
    /// `<details>` for power users; TV doesn't show it.
    pub nfo: Option<String>,
    /// Server-side parsed view of the NFO so web + TV consume the same
    /// shape rather than each having to ship a `MediaInfo` regex tower.
    pub media_info: Option<MediaInfoSummary>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub category: Option<String>,
    pub uploader: Option<String>,
    #[serde(default)]
    pub uploaded_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Human-formatted age string from the provider when available
    /// (e.g., "6 minutes", "2 hours"). When absent the frontend can
    /// derive its own from `uploaded_at`.
    pub age: Option<String>,
    pub seeders: Option<u32>,
    pub leechers: Option<u32>,
    pub times_completed: Option<u64>,
    pub views: Option<u64>,
    #[serde(default)]
    pub freeleech: bool,
    #[serde(default)]
    pub exclusive: bool,
    pub file_count: Option<u32>,
    pub file_size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MediaInfoSummary {
    pub video: Option<VideoInfo>,
    #[serde(default)]
    pub audio: Vec<AudioInfo>,
    #[serde(default)]
    pub subtitles: Vec<SubInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoInfo {
    pub codec: Option<String>,
    /// `"1920x1080"` style.
    pub resolution: Option<String>,
    pub duration_secs: Option<u32>,
    pub fps: Option<f32>,
    pub bitrate_kbps: Option<u32>,
    /// `"HDR10"`, `"Dolby Vision"`, `"SDR"` etc. Best-effort heuristic.
    pub hdr: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioInfo {
    pub lang: Option<String>,
    pub codec: Option<String>,
    pub channels: Option<u8>,
    pub bitrate_kbps: Option<u32>,
    pub title: Option<String>,
    #[serde(default)]
    pub default: bool,
    /// Atmos / Dolby commercial name when `MediaInfo` flagged it.
    pub commercial_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubInfo {
    pub lang: Option<String>,
    /// `"UTF-8"`, `"PGS"`, `"VobSub"`, etc.
    pub format: Option<String>,
    pub title: Option<String>,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub forced: bool,
}
