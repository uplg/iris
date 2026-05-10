//! Library views — the user-facing browse surface.
//!
//! Two views over the same underlying data:
//!   * **collections** (default, family-friendly): one card per logical
//!     library entity (TV show with N torrents → 1 collection card).
//!   * **torrents** (admin / power-user): the raw torrent list, the
//!     way the legacy `/api/torrents` endpoint exposes it.
//!
//! The frontend toggles between them via `?view=`. Collections is the
//! one chérie / mom see; torrents stays available for the seedbox UI
//! at `/admin`.

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::AuthUser;
use crate::routes::torrents::TorrentView;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_library))
        .route("/collections/{id}", get(collection_detail))
}

#[derive(Debug, Deserialize, Default)]
struct LibraryQuery {
    /// `"collections"` (default) or `"torrents"`. Anything else is
    /// treated as the default — easier than rejecting weird values.
    #[serde(default)]
    view: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "view")]
enum LibraryResponse {
    #[serde(rename = "collections")]
    Collections { items: Vec<CollectionListItem> },
    #[serde(rename = "torrents")]
    Torrents {
        items: Vec<TorrentView>,
        /// Lifetime upload across every torrent ever ingested, including
        /// the soft-deleted ones not present in `items`. Lets the raw
        /// view show the "since the beginning" total without needing
        /// admin scope (the admin storage endpoint does the same sum).
        total_uploaded_bytes: u64,
    },
}

#[derive(Debug, Serialize)]
struct CollectionListItem {
    id: Uuid,
    tmdb_id: Option<i64>,
    display_title: String,
    kind: String,
    /// Number of torrents currently attached. ≥ 1 — the listing already
    /// filters out empty collections.
    torrent_count: i64,
    total_size_bytes: i64,
    /// Distinct (season, episode) pairs we have on disk. Drives the
    /// "X / Y épisodes" subtitle on the collection card.
    episode_count: i64,
    /// Fallback nav target for clients that lack the rich Series page
    /// routing (or for collections without a `tmdb_id`). Always present
    /// when `torrent_count > 0`.
    representative_infohash: Option<String>,
}

async fn list_library(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<LibraryQuery>,
) -> ApiResult<Json<LibraryResponse>> {
    if q.view.as_deref() == Some("torrents") {
        let rows = iris_db::torrents::list_active(state.db()).await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            if let Some(snapshot) = state.engine().get_by_infohash(&row.infohash) {
                out.push(TorrentView {
                    id: row.id,
                    added_by: row.added_by,
                    added_by_name: row.added_by_name,
                    added_at: row.added_at,
                    last_played_at: row.last_played_at,
                    source_provider: row.source_provider,
                    source_external_id: row.source_external_id,
                    tmdb_id: row.tmdb_id,
                    tmdb_verified: row.tmdb_verified,
                    kind: row.kind,
                    uploaded_bytes_total: u64::try_from(row.uploaded_bytes_total).unwrap_or(0),
                    snapshot,
                });
            }
        }
        let _ = user; // keep the auth gate, no per-user filtering yet
        let total_uploaded_bytes = iris_db::torrents::total_uploaded_bytes(state.db())
            .await
            .unwrap_or(0);
        return Ok(Json(LibraryResponse::Torrents {
            items: out,
            total_uploaded_bytes,
        }));
    }
    // Default: collections.
    let summaries = iris_db::collections::list_summaries(state.db()).await?;
    let items = summaries
        .into_iter()
        .map(|s| CollectionListItem {
            id: s.id,
            tmdb_id: s.tmdb_id,
            display_title: s.display_title,
            kind: s.kind,
            torrent_count: s.torrent_count,
            total_size_bytes: s.total_size_bytes,
            episode_count: s.episode_count,
            representative_infohash: s.representative_infohash,
        })
        .collect();
    Ok(Json(LibraryResponse::Collections { items }))
}

#[derive(Debug, Serialize)]
struct CollectionDetail {
    id: Uuid,
    tmdb_id: Option<i64>,
    display_title: String,
    kind: String,
    /// All torrents attached to this collection.
    torrents: Vec<TorrentView>,
    /// Merged episode list across every torrent in the collection.
    /// Empty for movie-kind collections (which usually have a single
    /// torrent + a single video file).
    episodes: Vec<EpisodeEntry>,
}

#[derive(Debug, Serialize)]
struct EpisodeEntry {
    season: i64,
    episode: i64,
    infohash: String,
    file_idx: i64,
    /// `true` when the requesting user's `playback_progress.completed`
    /// is set for this file — drives the "vu" badge on the Series page.
    watched: bool,
}

async fn collection_detail(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<CollectionDetail>> {
    let collection = iris_db::collections::get(state.db(), id)
        .await?
        .ok_or(ApiError::NotFound)?;

    let torrent_rows = iris_db::torrents::list_in_collection(state.db(), id).await?;
    let mut torrents = Vec::with_capacity(torrent_rows.len());
    for row in &torrent_rows {
        if let Some(snapshot) = state.engine().get_by_infohash(&row.infohash) {
            torrents.push(TorrentView {
                id: row.id,
                added_by: row.added_by,
                added_by_name: row.added_by_name.clone(),
                added_at: row.added_at,
                last_played_at: row.last_played_at,
                source_provider: row.source_provider.clone(),
                source_external_id: row.source_external_id.clone(),
                tmdb_id: row.tmdb_id,
                tmdb_verified: row.tmdb_verified,
                kind: row.kind.clone(),
                uploaded_bytes_total: u64::try_from(row.uploaded_bytes_total).unwrap_or(0),
                snapshot,
            });
        }
    }

    let episodes = if collection.kind == "tv" {
        // SCENE-first: episode_files joins on collection_id directly,
        // no need to bridge through tmdb_id (which may be unset on
        // SCENE-only collections).
        let files = iris_db::episode_files::list_for_collection(state.db(), collection.id).await?;
        let mut out: Vec<EpisodeEntry> = Vec::with_capacity(files.len());
        for f in files {
            let watched = iris_db::playback::get(state.db(), user.id, &f.infohash, f.file_idx)
                .await
                .unwrap_or(None)
                .is_some_and(|p| p.completed);
            out.push(EpisodeEntry {
                season: f.season,
                episode: f.episode,
                infohash: f.infohash,
                file_idx: f.file_idx,
                watched,
            });
        }
        out.sort_by_key(|e| (e.season, e.episode));
        out
    } else {
        Vec::new()
    };

    Ok(Json(CollectionDetail {
        id: collection.id,
        tmdb_id: collection.tmdb_id,
        display_title: collection.display_title,
        kind: collection.kind,
        torrents,
        episodes,
    }))
}
