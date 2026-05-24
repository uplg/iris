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
        .route(
            "/collections/{id}/grab/{season}/{episode}",
            axum::routing::post(grab_collection_episode),
        )
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
    /// Language tag derived from the parent torrent's SCENE name so
    /// users can tell a French / English / `MULTi` episode apart at a
    /// glance. Same string form as `AvailableEpisodeEntry.language`
    /// (`"french"` / `"english"` / `"multi"` / `"unknown"`). `null`
    /// when the parent torrent is no longer registered in the
    /// engine (shouldn't happen but defensive).
    language: Option<String>,
}

#[derive(Debug, Serialize)]
struct AvailableEpisodeEntry {
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
}

/// Season-pack offer the indexer scanner cached for this collection.
/// Surfaced as its own list (separate from `available_episodes`) so
/// the UI can render a "Grab full Season N" CTA instead of trying
/// to display the pack as a single episode row. Grab path
/// transparently falls back to the matching pack when a user clicks
/// a missing per-episode (S, E) that no singleton offers.
#[derive(Debug, Serialize)]
struct SeasonPackEntry {
    season: i64,
    indexer_provider: String,
    indexer_torrent_id: String,
    quality: Option<String>,
    seeders: Option<i64>,
    size_bytes: Option<i64>,
    found_at: DateTime<Utc>,
    language: Option<String>,
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
    let user_last_visited: Option<DateTime<Utc>> = match collection.parsed_title_normalized.as_deref() {
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
    if let Some(norm) = collection.parsed_title_normalized.as_deref() {
        if let Ok(Some(row)) =
            iris_db::follows::get_by_normalized(state.db(), user.id, norm).await
        {
            let _ = iris_db::follows::mark_visited(state.db(), user.id, row.id).await;
        }
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

    Ok(Json(CollectionDetail {
        id: collection.id,
        tmdb_id: collection.tmdb_id,
        display_title: collection.display_title,
        kind: collection.kind,
        poster_path,
        backdrop_path,
        torrents,
        episodes,
        available_episodes,
        season_packs,
        has_new_since_last_visit,
    }))
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
        if owned_torrent_ids
            .contains(&(o.indexer_provider.clone(), o.indexer_torrent_id.clone()))
        {
            continue;
        }
        let offer_lang =
            iris_media::filename::Language::parse_tag(o.language.as_deref().unwrap_or(""));
        let covered = owned_languages.get(&(o.season, o.episode)).is_some_and(|owned| {
            owned
                .iter()
                .any(|&l| l == iris_media::filename::Language::Multi || l == offer_lang)
        });
        if covered {
            continue;
        }
        // Dead offer (0 seeders, or unknown count): undownloadable
        // and only clutters the grid. Pack offers stay regardless —
        // a quiet pack still beats no pack when no singletons exist.
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
    let mut owned_languages: std::collections::HashMap<(i64, i64), Vec<iris_media::filename::Language>> =
        std::collections::HashMap::new();
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
        // Same resolver as above so a chip's badge agrees with the
        // owned_languages dedup map — keeps the UI from disagreeing
        // with the server about whether an offer's language is
        // already covered.
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
        let packs = iris_db::available_episodes::list_season_packs_for_series(state.db(), normalized)
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
#[derive(Debug, Deserialize)]
struct GrabQuery {
    #[serde(default)]
    language: Option<String>,
}

async fn grab_collection_episode(
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
