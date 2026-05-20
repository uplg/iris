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
    /// SCENE-parsed title from `q`, lowercased + punctuation stripped.
    /// `None` when `q` doesn't parse to a usable title. Providers may
    /// ignore this; the API layer uses it for relevance scoring.
    #[serde(default)]
    pub parsed_title: Option<String>,
    /// SCENE-parsed season number from `q`. `Some(0)` is the in-band
    /// sentinel for a season-pack query (e.g. `Show.Name.S04`).
    /// Torznab maps this to `season=`; UNIT3D/Torr9 append it to the
    /// name filter as `SxxExx` / `Sxx`.
    #[serde(default)]
    pub season: Option<u32>,
    /// SCENE-parsed episode number from `q`. Only set when `season`
    /// is also set and the parser found a full `SxxExx` marker.
    #[serde(default)]
    pub episode: Option<u32>,
    /// SCENE-parsed year from `q`. Used to disambiguate remakes
    /// (Dune 1984 vs Dune 2021) in the relevance score.
    #[serde(default)]
    pub year: Option<u16>,
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
    /// API-layer enrichment: `true` when the result's SCENE-parsed
    /// (title, season, episode) matches an existing `episode_files`
    /// row. UI uses this to disable the "Add to library" CTA and
    /// label the card as already-downloaded, preventing the
    /// surprisingly-common second ingest of the same episode under a
    /// different release group. Providers always emit `false`; the
    /// API search layer flips it before returning to the client.
    #[serde(default)]
    pub already_in_library: bool,
    /// Infohash of the existing library file when
    /// [`Self::already_in_library`] is `true`. Lets the UI link
    /// straight to `/watch/<infohash>/<file_idx>` instead of asking
    /// the user to re-download.
    #[serde(default)]
    pub library_infohash: Option<String>,
    /// File index inside [`Self::library_infohash`] for the matching
    /// episode. Paired with the infohash to build a direct
    /// `/watch/{infohash}/{file_idx}` URL.
    #[serde(default)]
    pub library_file_idx: Option<u32>,
    /// Coarse language tag derived server-side from the SCENE
    /// release name: `"french"` / `"english"` / `"multi"` /
    /// `"unknown"`. Drives the FR / EN / `MULTi` badges on
    /// search result cards so the household's anglophone users
    /// can spot a Seedpool release among the c411 / TOS
    /// francophones at a glance. Stable string form so
    /// deserialisers stay tolerant to future variants.
    #[serde(default)]
    pub language: Option<String>,
    /// Pre-signed `.torrent` download URL when the provider
    /// surfaces one in its search response (Torznab `<link>`,
    /// UNIT3D `download_link`). Internal only — not returned to
    /// clients (skipped on serialize). The scheduler persists it
    /// onto `available_episodes.download_url` so the grab path
    /// survives process restarts that wipe the in-memory link
    /// caches. `None` for providers that fetch URLs on demand
    /// (torr9's JSON API resolves per-id at grab time).
    #[serde(skip_serializing, default)]
    pub download_url: Option<String>,
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
/// What `TorrentDetails::description` is encoded in — indexers don't
/// agree on a format. The frontend dispatches the right renderer
/// (`BBCode` parser, sanitised HTML, raw text) off this. Defaults to
/// [`DescriptionFormat::Bbcode`] when absent so older provider payloads
/// (torr9 was the only source originally) keep working unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DescriptionFormat {
    /// torr9 dialect: `[b]`, `[center]`, `[size=N]`, `[color=#xxx]`,
    /// `[url=…]`, `[img]…[/img]`. Custom renderer in
    /// `web/src/components/PreviewDialog.tsx`.
    #[default]
    Bbcode,
    /// c411 dialect: full HTML (headings, tables, images). The frontend
    /// MUST sanitise with `DOMPurify` before rendering.
    Html,
    /// No markup — render verbatim in a `pre` block.
    Plain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentDetails {
    pub provider_id: String,
    pub external_id: String,
    pub title: String,
    /// Description body. Encoding depends on [`Self::description_format`].
    pub description: Option<String>,
    /// Encoding of `description`. Always set on new payloads; absent
    /// values default to [`DescriptionFormat::Bbcode`] for backward
    /// compatibility with pre-c411 clients.
    #[serde(default)]
    pub description_format: DescriptionFormat,
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
