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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TorrentSource {
    Magnet(String),
    TorrentFile(Vec<u8>),
}
