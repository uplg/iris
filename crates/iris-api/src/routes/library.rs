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
use chrono::{DateTime, Utc};
use iris_core::search::MediaKind;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::AuthUser;
use crate::routes::torrents::TorrentView;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_library))
        .route("/collections/{id}", get(collection_detail))
        .route(
            "/collections/{id}/grab/{season}/{episode}",
            axum::routing::post(grab_collection_episode),
        )
}

#[derive(Debug, Deserialize, Default, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct LibraryQuery {
    /// `"collections"` (default) or `"torrents"`. Anything else is
    /// treated as the default — easier than rejecting weird values.
    #[serde(default)]
    view: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "view")]
pub(crate) enum LibraryResponse {
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

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct CollectionListItem {
    id: Uuid,
    tmdb_id: Option<i64>,
    display_title: String,
    kind: MediaKind,
    /// `true` for anime collections (drives AniList-sourced metadata and
    /// keeps the anime / live-action split visible). Additive field —
    /// older clients ignore it.
    #[serde(default)]
    is_anime: bool,
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
    /// `true` for a GHOST collection: every torrent was reclaimed
    /// (disk GC / cleanup) but the REQUESTING user has watch history
    /// in it. Ghosts are per-caller (nobody sees another user's
    /// ghosts), render greyed-out, and stay navigable — the
    /// collection page still lists indexer offers, so the user can
    /// re-grab and resume exactly where they were. Additive — older
    /// clients render them as ordinary (empty) cards.
    #[serde(default)]
    ghost: bool,
}

#[utoipa::path(
    get,
    path = "/api/library",
    operation_id = "list_library",
    params(LibraryQuery),
    responses((status = 200, description = "Collections (default) or raw torrents", body = LibraryResponse)),
    tag = "library",
)]
pub(crate) async fn list_library(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<LibraryQuery>,
) -> ApiResult<Json<LibraryResponse>> {
    if q.view.as_deref() == Some("torrents") {
        let rows = iris_db::torrents::list_active(state.db()).await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            if let Some(snapshot) = state.engine().get_by_infohash(&row.infohash) {
                let tmdb_id = row.effective_tmdb_id();
                out.push(TorrentView {
                    id: row.id,
                    added_by: row.added_by,
                    added_by_name: row.added_by_name,
                    added_at: row.added_at,
                    last_played_at: row.last_played_at,
                    source_provider: row.source_provider,
                    source_external_id: row.source_external_id,
                    tmdb_id,
                    tmdb_verified: row.tmdb_verified,
                    kind: row
                        .kind
                        .as_deref()
                        .and_then(iris_core::search::MediaKind::from_wire),
                    collection_id: row.collection_id,
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
    // Default: collections — the shared live listing, plus the CALLER's
    // ghost collections (fully-GC'd shows/movies this user watched)
    // appended after. Ghosts are per-user by construction: they're
    // derived from the caller's own playback history, so Lyros' ghosts
    // never appear in anyone else's Library.
    let summaries = iris_db::collections::list_summaries(state.db()).await?;
    let ghosts = iris_db::collections::list_ghost_summaries_for_user(state.db(), user.id)
        .await
        .unwrap_or_default();
    let to_item = |s: iris_db::collections::CollectionSummary, ghost: bool| CollectionListItem {
        id: s.id,
        tmdb_id: s.tmdb_id,
        display_title: s.display_title,
        // `collections.kind` is `NOT NULL CHECK (kind IN ('tv','movie'))`,
        // so `from_wire` only ever returns `None` on a corrupt row.
        kind: MediaKind::from_wire(&s.kind).unwrap_or(MediaKind::Tv),
        is_anime: s.is_anime,
        torrent_count: s.torrent_count,
        total_size_bytes: s.total_size_bytes,
        episode_count: s.episode_count,
        representative_infohash: s.representative_infohash,
        ghost,
    };
    let items = summaries
        .into_iter()
        .map(|s| to_item(s, false))
        .chain(ghosts.into_iter().map(|s| to_item(s, true)))
        .collect();
    Ok(Json(LibraryResponse::Collections { items }))
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct CollectionDetail {
    id: Uuid,
    tmdb_id: Option<i64>,
    display_title: String,
    kind: MediaKind,
    /// `true` for anime collections. Additive — older clients ignore it.
    #[serde(default)]
    is_anime: bool,
    /// How the client should lay out episodes: `"seasonal"` (the
    /// default — season tabs) or `"absolute"` (one flat ordered
    /// "Episode N" list, for fleuve anime whose releases cram the
    /// absolute number into a fake `S01`). Derived from the episode set,
    /// NOT from `is_anime`: a season-cut anime stays `"seasonal"`.
    /// Additive — older clients ignore it and keep season tabs.
    #[serde(default = "default_numbering")]
    numbering: String,
    /// Server-resolved poster path (TMDB convention — pass through
    /// `tmdbImage(path, size)` client-side). `None` when no TMDB id
    /// is attached or the lookup fails. Looked up here (rather than
    /// from the client) so every collection-detail render gets a
    /// poster without an extra round-trip and a separate
    /// `/api/metadata/tmdb/:id` call.
    #[serde(default)]
    poster_path: Option<String>,
    #[serde(default)]
    backdrop_path: Option<String>,
    /// All torrents attached to this collection.
    torrents: Vec<TorrentView>,
    /// Merged episode list across every torrent in the collection.
    /// Empty for movie-kind collections (which usually have a single
    /// torrent + a single video file).
    episodes: Vec<EpisodeEntry>,
    /// Indexer-cached episode offers for this collection (TV only).
    /// Already-in-library episodes are filtered out so this list is
    /// "what the user could grab next" — the page renders them as
    /// grabbable rows alongside the on-disk ones. Empty for movies
    /// and for collections with no SCENE identity. Additive field —
    /// pre-0.4 clients ignore it.
    #[serde(default)]
    available_episodes: Vec<AvailableEpisodeEntry>,
    /// Season-pack offers cached for this collection. The UI shows
    /// these as a separate "Grab full Season N" CTA (not as
    /// per-episode rows). The grab path also consults these when a
    /// user clicks a (S, E) that has no singleton offer — the pack
    /// gets ingested and the matching leaf is returned.
    #[serde(default)]
    season_packs: Vec<SeasonPackEntry>,
    /// Count of `available_episodes` whose `found_at >
    /// last_visited_at`. Drives the home-page Watchlist "X new"
    /// badge. Computed before `last_visited_at` is bumped to now.
    /// `0` for movies / no-SCENE collections.
    #[serde(default)]
    has_new_since_last_visit: u32,
    /// Releases that used to be on disk (reclaimed by the GC / a
    /// cleanup) and can be re-grabbed from their source indexer.
    /// This is the ghost-resume path for MOVIES — TV additionally has
    /// `available_episodes` — but is populated for both kinds:
    /// re-resolving the same release yields the same infohash, so any
    /// saved playback position resumes untouched. Only releases whose
    /// source provenance survived are listed (the actionable ones),
    /// minus the ones the CALLER dismissed. Additive — older clients
    /// ignore it.
    #[serde(default)]
    gone_releases: Vec<GoneReleaseEntry>,
    /// The ghost twin of `episodes`: every (season, episode) row whose
    /// source release was reclaimed, with the CALLER's watch state
    /// attached. New clients merge these into the episode list so a
    /// ghost collection renders exactly as it did before the GC — same
    /// rows, same "already watched" badges — with "Download again"
    /// instead of Play. Kept OUT of `episodes` on purpose: an old
    /// client must never offer Play on a deleted torrent. Rows the
    /// caller dismissed are excluded. Additive — older clients ignore
    /// it and keep the flat `gone_releases` list.
    #[serde(default)]
    gone_episodes: Vec<GoneEpisodeEntry>,
}

/// One reclaimed release the user can re-download from the collection
/// page. `source_provider` + `source_external_id` feed the existing
/// ingest endpoint (`POST /api/torrents`) unchanged.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct GoneReleaseEntry {
    infohash: String,
    name: String,
    source_provider: String,
    source_external_id: String,
    total_size_bytes: i64,
    /// When the GC / cleanup reclaimed it. Additive — `None` only on
    /// legacy rows that predate `deleted_at` stamping (shouldn't happen).
    #[serde(default)]
    deleted_at: Option<DateTime<Utc>>,
    /// CALLER's watch state on this release (most recent file — the
    /// meaningful one for a single-file movie): `watched` mirrors
    /// `playback_progress.completed`, the rest carries the mid-way
    /// resume position like a History row. All additive.
    #[serde(default)]
    watched: bool,
    #[serde(default)]
    position_seconds: Option<f64>,
    #[serde(default)]
    duration_seconds: Option<f64>,
    #[serde(default)]
    last_watched_at: Option<DateTime<Utc>>,
}

/// One episode whose source release was reclaimed — the ghost twin of
/// [`EpisodeEntry`], plus the caller's watch state and the re-grab
/// provenance. `(infohash, file_idx)` stays the identity key, exactly
/// like `episodes` (a mis-parsed pack's colliding (S, E) rows must all
/// survive — see the identity contract on [`build_tv_episode_view`]).
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct GoneEpisodeEntry {
    season: i64,
    episode: i64,
    /// Absolute episode number for fleuve anime (render "Episode N").
    #[serde(default)]
    absolute_episode: Option<i64>,
    infohash: String,
    file_idx: i64,
    /// `true` when the requesting user's `playback_progress.completed`
    /// is set for this exact file — the "already watched" badge.
    watched: bool,
    /// Mid-way resume state (same semantics as a History row); all
    /// `None` when the caller never started this file.
    #[serde(default)]
    position_seconds: Option<f64>,
    #[serde(default)]
    duration_seconds: Option<f64>,
    #[serde(default)]
    last_watched_at: Option<DateTime<Utc>>,
    /// Same string form as `EpisodeEntry.language` (`"french"` /
    /// `"english"` / `"multi"` / `"unknown"`), derived from the
    /// reclaimed torrent's SCENE name.
    language: Option<String>,
    /// Raw SCENE name of the reclaimed release — secondary display line.
    release_name: String,
    /// Provenance feeding the existing ingest endpoint
    /// (`POST /api/torrents`) — always present: rows without it are not
    /// actionable and are filtered out server-side.
    source_provider: String,
    source_external_id: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct EpisodeEntry {
    season: i64,
    episode: i64,
    infohash: String,
    file_idx: i64,
    /// `true` when the requesting user's `playback_progress.completed`
    /// is set for this file — drives the "vu" badge on the Series page.
    watched: bool,
    /// Language tag derived from the parent torrent's SCENE name so
    /// users can tell a French / English / `MULTi` episode apart at a
    /// glance. Same string form as `AvailableEpisodeEntry.language`
    /// (`"french"` / `"english"` / `"multi"` / `"unknown"`). `null`
    /// when the parent torrent is no longer registered in the
    /// engine (shouldn't happen but defensive).
    language: Option<String>,
    /// Absolute episode number for fleuve anime (`One Piece S01E1156` →
    /// 1156). `null` for ordinary seasonal episodes. The client renders
    /// "Episode N" from this when the collection's `numbering` is
    /// `"absolute"`. Additive — older clients ignore it.
    #[serde(default)]
    absolute_episode: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct AvailableEpisodeEntry {
    season: i64,
    episode: i64,
    indexer_provider: String,
    indexer_torrent_id: String,
    quality: Option<String>,
    seeders: Option<i64>,
    size_bytes: Option<i64>,
    found_at: DateTime<Utc>,
    /// `"french"` / `"english"` / `"multi"` / `"unknown"` —
    /// stable string form. Clients render an FR / EN / `MULTi`
    /// badge per row so anglophone users can spot Seedpool
    /// releases at a glance. `null` only on legacy DB rows from
    /// before migration 0017 (they read as "unknown" downstream).
    language: Option<String>,
    /// Absolute episode number for fleuve anime offers. `null` for
    /// seasonal releases. Additive — older clients ignore it.
    #[serde(default)]
    absolute_episode: Option<i64>,
}

/// Season-pack offer the indexer scanner cached for this collection.
/// Surfaced as its own list (separate from `available_episodes`) so
/// the UI can render a "Grab full Season N" CTA instead of trying
/// to display the pack as a single episode row. Grab path
/// transparently falls back to the matching pack when a user clicks
/// a missing per-episode (S, E) that no singleton offers.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct SeasonPackEntry {
    season: i64,
    indexer_provider: String,
    indexer_torrent_id: String,
    quality: Option<String>,
    seeders: Option<i64>,
    size_bytes: Option<i64>,
    found_at: DateTime<Utc>,
    language: Option<String>,
}

/// serde default for the additive `numbering` field — absent on old
/// payloads, which must keep their season-tab layout.
fn default_numbering() -> String {
    "seasonal".to_string()
}

/// Decide the client episode layout from the evidence, NOT from
/// `is_anime`: `"absolute"` when the absolute-numbered (fleuve) episodes
/// dominate, else `"seasonal"`. On-disk episodes (what's actually in the
/// library) drive the call; a fully-reclaimed (ghost) collection falls
/// back to its GONE episodes so the layout survives the GC; cached
/// offers only break the tie when neither exists. A season-cut anime —
/// whose episodes carry no absolute number — correctly stays
/// `"seasonal"`.
fn derive_numbering(
    episodes: &[EpisodeEntry],
    gone_episodes: &[GoneEpisodeEntry],
    available: &[AvailableEpisodeEntry],
) -> String {
    // (episode, has-absolute) pairs, skipping the season-pack sentinel
    // (`episode == 0`).
    fn tally(pairs: impl Iterator<Item = (i64, bool)>) -> (usize, usize) {
        let (mut total, mut absolute) = (0usize, 0usize);
        for (episode, has_absolute) in pairs {
            if episode == 0 {
                continue;
            }
            total += 1;
            if has_absolute {
                absolute += 1;
            }
        }
        (total, absolute)
    }
    let mut counts = tally(episodes.iter().map(|e| (e.episode, e.absolute_episode.is_some())));
    if counts.0 == 0 {
        counts = tally(
            gone_episodes
                .iter()
                .map(|e| (e.episode, e.absolute_episode.is_some())),
        );
    }
    if counts.0 == 0 {
        counts = tally(available.iter().map(|a| (a.episode, a.absolute_episode.is_some())));
    }
    let (total, absolute) = counts;
    if total > 0 && absolute * 2 >= total {
        "absolute".to_string()
    } else {
        default_numbering()
    }
}

#[utoipa::path(
    get,
    path = "/api/library/collections/{id}",
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, description = "Collection detail with episodes + offers", body = CollectionDetail),
        (status = 404, description = "Unknown collection"),
    ),
    tag = "library",
)]
pub(crate) async fn collection_detail(
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
                tmdb_id: row.effective_tmdb_id(),
                tmdb_verified: row.tmdb_verified,
                kind: row
                    .kind
                    .as_deref()
                    .and_then(iris_core::search::MediaKind::from_wire),
                collection_id: row.collection_id,
                uploaded_bytes_total: u64::try_from(row.uploaded_bytes_total).unwrap_or(0),
                snapshot,
            });
        }
    }

    // Per-user "X new since I last opened this" timestamp. Pulled
    // from this user's series_follows row (auto-created by the
    // grab path); falls back to None when the user has never
    // touched this series — in which case every available episode
    // counts as new.
    let user_last_visited: Option<DateTime<Utc>> =
        match collection.parsed_title_normalized.as_deref() {
            Some(norm) => iris_db::follows::get_by_normalized(state.db(), user.id, norm)
                .await
                .ok()
                .flatten()
                .and_then(|f| f.last_visited_at),
            None => None,
        };

    let (episodes, available_episodes, season_packs, has_new_since_last_visit) =
        if collection.kind == "tv" {
            build_tv_episode_view(&state, &collection, user.id, user_last_visited).await?
        } else {
            (Vec::new(), Vec::new(), Vec::new(), 0)
        };

    // Bump *this user's* visited timestamp when (and only when) the
    // user is already tracking this series. We deliberately don't
    // auto-create the follow row here: opening a collection page to
    // browse isn't a strong enough signal — auto-tracking belongs to
    // the grab path. Without the row → no badge to bump, nothing to
    // do. The collection-wide `last_visited_at` column stays unused
    // (kept around for the v0.5 cleanup).
    if let Some(norm) = collection.parsed_title_normalized.as_deref()
        && let Ok(Some(row)) = iris_db::follows::get_by_normalized(state.db(), user.id, norm).await
    {
        let _ = iris_db::follows::mark_visited(state.db(), user.id, row.id).await;
    }

    // TMDB lookup for the hero poster — same gating as the
    // Watchlist endpoint: only fires when a tmdb_id is attached.
    // Hand TMDB the collection's `kind` so it queries the right
    // namespace: `/tv/60573` vs `/movie/60573` are two unrelated
    // entries and a hint-less lookup serves whichever wins the
    // fallback coin-flip (the entire reason `lookup_with_kind`
    // exists).
    let kind_hint = match collection.kind.as_str() {
        "tv" => Some(crate::tmdb::TmdbKind::Tv),
        "movie" => Some(crate::tmdb::TmdbKind::Movie),
        _ => None,
    };
    let (poster_path, backdrop_path) = match (state.tmdb(), collection.tmdb_id) {
        (Some(client), Some(tid)) => {
            #[allow(clippy::cast_sign_loss)]
            let meta = client.lookup_with_kind(tid as u64, kind_hint).await;
            meta.map_or((None, None), |m| (m.poster_path, m.backdrop_path))
        }
        _ => (None, None),
    };

    let (gone_releases, gone_episodes) = build_gone_view(&state, id, user.id).await;

    let numbering = derive_numbering(&episodes, &gone_episodes, &available_episodes);
    Ok(Json(CollectionDetail {
        id: collection.id,
        tmdb_id: collection.tmdb_id,
        display_title: collection.display_title,
        // `collections.kind` is CHECK-constrained to 'tv'/'movie'.
        kind: MediaKind::from_wire(&collection.kind).unwrap_or(MediaKind::Tv),
        is_anime: collection.is_anime,
        numbering,
        poster_path,
        backdrop_path,
        torrents,
        episodes,
        available_episodes,
        season_packs,
        has_new_since_last_visit,
        gone_releases,
        gone_episodes,
    }))
}

/// The per-caller "gone" view of a collection: reclaimed releases with
/// surviving provenance (the "Download again" list — movies especially:
/// without it a ghost movie collection is a dead end, having no
/// `available_episodes` to re-grab) and the ghost twin of `episodes`
/// (reclaimed (S, E) rows), both enriched with the CALLER's watch state
/// so the page renders exactly as it did before the GC. Releases the
/// caller dismissed are already filtered out at the query level.
async fn build_gone_view(
    state: &AppState,
    collection_id: Uuid,
    user_id: iris_core::ids::UserId,
) -> (Vec<GoneReleaseEntry>, Vec<GoneEpisodeEntry>) {
    let deleted_rows =
        iris_db::torrents::list_deleted_in_collection(state.db(), collection_id, user_id)
            .await
            .unwrap_or_default();
    // Caller's playback rows on the reclaimed torrents, newest first —
    // `find` therefore picks the most recent file's state per release
    // (the meaningful one for a single-file movie).
    let gone_watch =
        iris_db::playback::watch_state_for_deleted_in_collection(state.db(), user_id, collection_id)
            .await
            .unwrap_or_default();
    let gone_releases: Vec<GoneReleaseEntry> = deleted_rows
        .iter()
        .filter_map(|t| match (&t.source_provider, &t.source_external_id) {
            (Some(provider), Some(external_id)) => {
                let w = gone_watch.iter().find(|w| w.infohash == t.infohash);
                Some(GoneReleaseEntry {
                    infohash: t.infohash.clone(),
                    name: t.name.clone(),
                    source_provider: provider.clone(),
                    source_external_id: external_id.clone(),
                    total_size_bytes: t.total_size_bytes,
                    deleted_at: t.deleted_at,
                    watched: w.is_some_and(|w| w.completed),
                    position_seconds: w.map(|w| w.position_seconds),
                    duration_seconds: w.and_then(|w| w.duration_seconds),
                    last_watched_at: w.map(|w| w.last_watched_at),
                })
            }
            _ => None,
        })
        .collect();

    // Language detection reuses the SCENE-name resolver with maps built
    // from the DELETED torrent rows (the live-torrent maps in
    // `build_tv_episode_view` don't know these infohashes).
    let gone_names: std::collections::HashMap<String, String> = deleted_rows
        .iter()
        .map(|t| (t.infohash.clone(), t.name.clone()))
        .collect();
    let gone_providers: std::collections::HashMap<String, String> = deleted_rows
        .iter()
        .filter_map(|t| t.source_provider.clone().map(|p| (t.infohash.clone(), p)))
        .collect();
    let gone_episodes: Vec<GoneEpisodeEntry> =
        iris_db::episode_files::list_gone_for_collection(state.db(), collection_id, user_id)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter_map(|r| {
                let (Some(provider), Some(external_id)) =
                    (r.source_provider, r.source_external_id)
                else {
                    return None; // no provenance → not re-grabbable
                };
                let language = Some(
                    resolve_torrent_language(state, &r.infohash, &gone_names, &gone_providers)
                        .as_str()
                        .to_string(),
                );
                Some(GoneEpisodeEntry {
                    season: r.season,
                    episode: r.episode,
                    absolute_episode: r.absolute_episode,
                    infohash: r.infohash,
                    file_idx: r.file_idx,
                    watched: r.completed,
                    position_seconds: r.position_seconds,
                    duration_seconds: r.duration_seconds,
                    last_watched_at: r.last_watched_at,
                    language,
                    release_name: r.torrent_name,
                    source_provider: provider,
                    source_external_id: external_id,
                })
            })
            .collect();
    (gone_releases, gone_episodes)
}

/// Resolve the language tag of an on-disk torrent. SCENE names are
/// the primary signal — `VOSTFR` / `MULTi` / `FRENCH` etc. are
/// detected from the torrent name. When the parser comes back
/// `Unknown` (Seedpool's "no explicit tag" convention is the typical
/// case), fall back to the source provider's `default_language`
/// config so Seedpool ingests get tagged English instead of staying
/// "unknown" and silently breaking the language-dedup filter.
fn resolve_torrent_language(
    state: &AppState,
    infohash: &str,
    torrent_names: &std::collections::HashMap<String, String>,
    torrent_source_providers: &std::collections::HashMap<String, String>,
) -> iris_media::filename::Language {
    let detected = torrent_names
        .get(infohash)
        .map_or(iris_media::filename::Language::Unknown, |n| {
            iris_media::filename::detect_language(n)
        });
    if detected != iris_media::filename::Language::Unknown {
        return detected;
    }
    torrent_source_providers
        .get(infohash)
        .and_then(|p| state.providers().default_language(p))
        .map_or(detected, iris_media::filename::Language::parse_tag)
}

/// Project the scheduler's cached `available_episodes` rows into
/// the API response shape: drop owned-language duplicates, drop
/// 0-seeder corpses, count what's "new since last visit". Extracted
/// from `build_tv_episode_view` to keep that function under the
/// clippy line cap. Language coverage rule: an owned `Multi`
/// release satisfies any language (Multi packs both audio tracks),
/// so it shadows every offer for that (S, E).
async fn build_available_singletons(
    state: &AppState,
    normalized: &str,
    owned_languages: &std::collections::HashMap<(i64, i64), Vec<iris_media::filename::Language>>,
    owned_torrent_ids: &std::collections::HashSet<(String, String)>,
    user_last_visited: Option<DateTime<Utc>>,
) -> (Vec<AvailableEpisodeEntry>, u32) {
    let offers = iris_db::available_episodes::list_best_for_series(state.db(), normalized)
        .await
        .unwrap_or_default();
    let mut out: Vec<AvailableEpisodeEntry> = Vec::new();
    let mut new_count: u32 = 0;
    for o in offers {
        // `list_best_for_series` already excludes packs at the SQL
        // level, but belt + braces against stale rows.
        if o.episode == 0 {
            continue;
        }
        // Exact-identity match: the offer's torrent is already in
        // the collection (just-grabbed, still downloading, or fully
        // on disk). Surfacing it as a "Grab" chip would let the
        // user retrigger the same ingest from the same row. Skip.
        if owned_torrent_ids.contains(&(o.indexer_provider.clone(), o.indexer_torrent_id.clone())) {
            continue;
        }
        let offer_lang =
            iris_media::filename::Language::parse_tag(o.language.as_deref().unwrap_or(""));
        let covered = owned_languages
            .get(&(o.season, o.episode))
            .is_some_and(|owned| {
                owned
                    .iter()
                    .any(|&l| l == iris_media::filename::Language::Multi || l == offer_lang)
            });
        if covered {
            continue;
        }
        // Dead offer (0 seeders, or unknown count): undownloadable
        // and only clutters the grid. Season packs are filtered
        // separately at the query source (`list_season_packs_for_series`
        // drops exactly-0-seeder packs), so this only governs singletons.
        if o.seeders.unwrap_or(0) <= 0 {
            continue;
        }
        if user_last_visited.is_none_or(|t| o.found_at > t) {
            new_count = new_count.saturating_add(1);
        }
        out.push(AvailableEpisodeEntry {
            season: o.season,
            episode: o.episode,
            indexer_provider: o.indexer_provider,
            indexer_torrent_id: o.indexer_torrent_id,
            quality: o.quality,
            seeders: o.seeders,
            size_bytes: o.size_bytes,
            found_at: o.found_at,
            language: o.language,
            absolute_episode: o.absolute_episode,
        });
    }
    out.sort_by_key(|e| (e.season, e.episode));
    (out, new_count)
}

/// Build the TV-shaped piece of the collection detail payload —
/// merged `episode_files` rows + indexer offers + per-user
/// new-since-last-visit count. Extracted from `collection_detail`
/// to keep that handler under the clippy line cap.
///
/// Identity contract: each `EpisodeEntry` is one physical file,
/// uniquely identified by `(infohash, file_idx)` (DB `UNIQUE`
/// constraint). We never dedup or drop a file here — a mis-parsed
/// pack whose leaves all collapsed onto the same `(season, episode)`
/// (e.g. the season-pack sentinel `episode == 0`) still surfaces
/// every file as its own playable row; the turn-7 reconcile
/// rewrites those rows to real numbers once the improved parser
/// re-derives them. Clients MUST key on `(infohash, file_idx)`,
/// not `(season, episode)` — the latter is derived and may collide.
#[allow(clippy::too_many_lines)] // one linear collection-view assembly
async fn build_tv_episode_view(
    state: &AppState,
    collection: &iris_db::collections::CollectionRow,
    user_id: iris_core::ids::UserId,
    user_last_visited: Option<DateTime<Utc>>,
) -> ApiResult<(
    Vec<EpisodeEntry>,
    Vec<AvailableEpisodeEntry>,
    Vec<SeasonPackEntry>,
    u32,
)> {
    // Read every torrent of the collection once — used below for
    // three things:
    //   * `owned_torrent_ids`: suppress any cached offer (singleton
    //     OR pack) whose indexer-side identity matches an
    //     already-ingested torrent. Without this you could see the
    //     same EN release as a "Grab" chip moments after kicking
    //     off its download.
    //   * `torrent_names`: per-episode SCENE language detection
    //     for the downloaded chip badge.
    //   * `torrent_source_providers`: fallback language when SCENE
    //     detection comes back `Unknown` (Seedpool ships English
    //     releases with no explicit tag — its provider config
    //     declares `default_language = "english"` and we honour
    //     that here).
    let collection_torrents = iris_db::torrents::list_in_collection(state.db(), collection.id)
        .await
        .unwrap_or_default();
    let owned_torrent_ids: std::collections::HashSet<(String, String)> = collection_torrents
        .iter()
        .filter_map(|t| match (&t.source_provider, &t.source_external_id) {
            (Some(p), Some(id)) => Some((p.clone(), id.clone())),
            _ => None,
        })
        .collect();
    let torrent_names: std::collections::HashMap<String, String> = collection_torrents
        .iter()
        .map(|t| (t.infohash.clone(), t.name.clone()))
        .collect();
    let torrent_source_providers: std::collections::HashMap<String, String> = collection_torrents
        .iter()
        .filter_map(|t| t.source_provider.clone().map(|p| (t.infohash.clone(), p)))
        .collect();
    let files = iris_db::episode_files::list_for_collection(state.db(), collection.id).await?;
    let mut episodes_out: Vec<EpisodeEntry> = Vec::with_capacity(files.len());
    // Per-(season, episode) owned-language map: for each owned
    // episode, which release languages are already on disk? Used
    // below to filter `available_episodes` precisely instead of
    // dropping every variant the moment one language is owned —
    // a user with the FR release of S01E01 should still see EN /
    // MULTi variants as grabable alternatives.
    let mut owned_languages: std::collections::HashMap<
        (i64, i64),
        Vec<iris_media::filename::Language>,
    > = std::collections::HashMap::new();
    for f in files {
        // episode == 0 is the season-pack sentinel — keep it out
        // of the dedup map so the indexer's individual S04E05
        // hit still surfaces as "available" even when an S04
        // pack has been ingested.
        if f.episode > 0 {
            let lang = resolve_torrent_language(
                state,
                &f.infohash,
                &torrent_names,
                &torrent_source_providers,
            );
            owned_languages
                .entry((f.season, f.episode))
                .or_default()
                .push(lang);
        }
        let watched = iris_db::playback::get(state.db(), user_id, &f.infohash, f.file_idx)
            .await
            .unwrap_or(None)
            .is_some_and(|p| p.completed);
        let language = Some(
            resolve_torrent_language(
                state,
                &f.infohash,
                &torrent_names,
                &torrent_source_providers,
            )
            .as_str()
            .to_string(),
        );
        episodes_out.push(EpisodeEntry {
            season: f.season,
            episode: f.episode,
            infohash: f.infohash,
            file_idx: f.file_idx,
            watched,
            language,
            absolute_episode: f.absolute_episode,
        });
    }
    episodes_out.sort_by_key(|e| (e.season, e.episode, e.file_idx));

    // Indexer offers for this collection's SCENE identity.
    // No identity → no offers (the long-tail standalone path).
    let mut available_out: Vec<AvailableEpisodeEntry> = Vec::new();
    let mut packs_out: Vec<SeasonPackEntry> = Vec::new();
    let mut new_count: u32 = 0;
    if let Some(normalized) = collection.parsed_title_normalized.as_deref() {
        (available_out, new_count) = build_available_singletons(
            state,
            normalized,
            &owned_languages,
            &owned_torrent_ids,
            user_last_visited,
        )
        .await;

        // Season packs surfaced separately. The UI renders them as
        // a "Grab full Season N" affordance; the grab path also
        // consults them when a user clicks a (S, E) with no
        // singleton offer. Already-grabbed packs are filtered out
        // — once the user has a pack in the library every leaf
        // has an `episode_files` row and the banner would just
        // nudge them to re-download what they already own.
        //
        // Redundant-language packs (FR/EN pack when a Multi episode
        // release is already owned, etc.) are filtered too — collapse
        // `owned_languages` (per-episode) down to per-season language
        // coverage from what's actually on disk, not from the indexer's
        // offer cache (a season grabbed as a single Multi pack has no
        // `episode > 0` cache rows to derive coverage from otherwise).
        let owned_season_coverage = season_language_coverage(&owned_languages);
        let packs = iris_db::available_episodes::list_season_packs_for_series(
            state.db(),
            normalized,
            &owned_season_coverage,
        )
        .await
        .unwrap_or_default();
        for p in packs {
            if owned_torrent_ids
                .contains(&(p.indexer_provider.clone(), p.indexer_torrent_id.clone()))
            {
                continue;
            }
            packs_out.push(SeasonPackEntry {
                season: p.season,
                indexer_provider: p.indexer_provider,
                indexer_torrent_id: p.indexer_torrent_id,
                quality: p.quality,
                seeders: p.seeders,
                size_bytes: p.size_bytes,
                found_at: p.found_at,
                language: p.language,
            });
        }
        packs_out.sort_by_key(|p| (p.season, p.language.clone().unwrap_or_default()));
    }
    Ok((episodes_out, available_out, packs_out, new_count))
}

/// Collapses a per-`(season, episode)` owned-language map down to
/// per-season language coverage — feeds the redundant-season-pack filter
/// in [`list_season_packs_for_series`](iris_db::available_episodes::list_season_packs_for_series).
fn season_language_coverage(
    owned_languages: &std::collections::HashMap<(i64, i64), Vec<iris_media::filename::Language>>,
) -> std::collections::HashMap<i64, std::collections::HashSet<String>> {
    let mut coverage: std::collections::HashMap<i64, std::collections::HashSet<String>> =
        std::collections::HashMap::new();
    for ((season, _episode), langs) in owned_languages {
        let entry = coverage.entry(*season).or_default();
        for lang in langs {
            entry.insert(lang.as_str().to_string());
        }
    }
    coverage
}

/// `POST /api/library/collections/:id/grab/:season/:episode` —
/// generalised grab endpoint that doesn't require a `series_follows`
/// row. The collection-driven Watchlist (post-0.4) calls this; the
/// legacy `/api/me/follows/:id/episodes/:s/:e/grab` route delegates
/// here too via the same shared `grab_episode_core` helper, so
/// behaviour is identical between the two surfaces.
///
/// Optional `?language=french|english|multi` narrows the picker to
/// one cached language slot — what the UI sends when the user
/// clicked a specific FR / EN badge. Without it the core falls
/// back to the historical "first available, any language" pick.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct GrabQuery {
    #[serde(default)]
    language: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/library/collections/{id}/grab/{season}/{episode}",
    operation_id = "grab_collection_episode",
    params(
        ("id" = Uuid, Path),
        ("season" = i64, Path),
        ("episode" = i64, Path),
        GrabQuery,
    ),
    responses(
        (status = 200, description = "Grabbed (or already-owned) episode", body = crate::routes::follows::GrabResponse),
        (status = 400, description = "Not a TV collection / no SCENE identity"),
        (status = 404, description = "Unknown collection"),
    ),
    tag = "library",
)]
pub(crate) async fn grab_collection_episode(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, season, episode)): Path<(Uuid, i64, i64)>,
    Query(q): Query<GrabQuery>,
) -> ApiResult<Json<crate::routes::follows::GrabResponse>> {
    let collection = iris_db::collections::get(state.db(), id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if collection.kind != "tv" {
        return Err(ApiError::BadRequest(
            "grab only valid for TV collections".into(),
        ));
    }
    let Some(normalized) = collection.parsed_title_normalized.as_deref() else {
        return Err(ApiError::BadRequest(
            "collection has no SCENE identity — cannot resolve episode".into(),
        ));
    };
    let resp = crate::routes::follows::grab_episode_core(
        &state,
        crate::routes::follows::GrabEpisodeRequest {
            user_id: user.id,
            normalized_name: normalized,
            display_title: &collection.display_title,
            tmdb_id: collection.tmdb_id,
            season,
            episode,
            language: crate::routes::follows::LangSel::from_badge(q.language.as_deref()),
        },
    )
    .await?;
    Ok(Json(resp))
}
