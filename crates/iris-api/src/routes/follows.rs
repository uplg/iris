// File-index / season / episode casts move between i64 (DB) and
// u32/u64 (engine / SCENE parser). Values are domain-bounded, so
// pedantic cast warnings are noise here.
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
)]

//! Per-user series-following endpoints. Mounted under `/api/me/follows`.
//!
//! Identity is the SCENE-normalised name. The Watchlist shelf and
//! Series page run entirely off this — TMDB is consulted only to
//! resolve a poster URL when the joined collection has been
//! `tmdb_verified` (probe runtime match).
//!
//! Episode listings come from two sources, keyed on the same
//! normalised name:
//!   * `episode_files` (via collection join) — what's on disk
//!   * `available_episodes` — what the indexer cached for "Préparer"

use std::collections::BTreeMap;

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use chrono::{DateTime, Utc};
use iris_media::filename::{Language, detect_language, series_key};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::AuthUser;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/episode-context", get(episode_context))
        .route("/{id}", delete(remove))
        .route("/{id}/episodes", get(episodes))
        .route(
            "/{id}/episodes/{season}/{episode}/grab",
            post(grab_episode),
        )
}

// ---------------------------------------------------------------------------
// POST /api/me/follows
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CreateFollowRequest {
    /// The display name from whatever surface the user clicked
    /// (Discovery / Search / `CollectionPage`). Server normalises it
    /// for identity; the original is kept for indexer queries and
    /// UI display.
    name: String,
    /// Optional TMDB id — stored as decoration. Surfaces a poster
    /// only after the corresponding collection gets `tmdb_verified`.
    tmdb_id: Option<i64>,
}

async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<CreateFollowRequest>,
) -> ApiResult<Json<FollowSummary>> {
    let trimmed = body.name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    let normalized = series_key(trimmed);
    if normalized.is_empty() {
        return Err(ApiError::BadRequest("name does not normalise".into()));
    }

    let row = iris_db::follows::add(
        state.db(),
        user.id,
        &normalized,
        trimmed,
        body.tmdb_id,
    )
    .await?;

    // Kick off an immediate background scan so the series page
    // shows `dispo` chips on first visit instead of waiting on the
    // periodic scheduler tick. Best-effort.
    //
    // The scheduler now keys on `collections`, not `series_follows`,
    // so we look up the TV collection that shares this SCENE
    // identity. The collection may not exist yet (no episode
    // ingested), in which case the scan no-ops — the scheduler will
    // pick it up the moment ingest creates a row.
    let pool = state.db().clone();
    let providers = state.providers().clone();
    let normalized = row.normalized_name.clone();
    let follow_name = row.name.clone();
    let follow_id = row.id;
    tokio::spawn(async move {
        let collection = match iris_db::collections::find_by_parsed_title(
            &pool,
            &normalized,
            iris_db::collections::Kind::Tv,
        )
        .await
        {
            Ok(Some(c)) => c,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!(
                    %follow_id,
                    %follow_name,
                    error = %e,
                    "follow create: collection lookup failed",
                );
                return;
            }
        };
        if let Err(e) =
            crate::collections_scheduler::scan_collection(&pool, &providers, collection.id).await
        {
            tracing::warn!(
                %follow_id,
                %follow_name,
                collection_id = %collection.id,
                error = %e,
                "follow create: initial scan failed",
            );
        }
    });

    Ok(Json(summarize(&state, &row).await))
}

// ---------------------------------------------------------------------------
// GET /api/me/follows
// ---------------------------------------------------------------------------
//
// C1 façade for APK 0.3.1: per-user Watchlist. With ~10 viewers from
// different families sharing one library, the Watchlist HAS to be
// per-user — series_follows is the right source. The "Follow"
// concept is no longer surfaced anywhere; rows are written
// automatically by `grab_episode_core` on every grab so the user's
// tracked-shows set just reflects what they actually download. APK
// 0.3.1 still calls this exactly the way it did — the only change
// is that the rows it now sees were created implicitly.
// The new web client calls `/api/me/watchlist` (same data, cleaner
// shape) and skips this façade entirely.

async fn list(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<FollowSummary>>> {
    let rows = iris_db::follows::list_for_user(state.db(), user.id).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(summarize(&state, &row).await);
    }
    Ok(Json(out))
}

#[derive(Debug, Serialize)]
struct FollowSummary {
    id: Uuid,
    /// SCENE-normalised name — clients route by this, not `tmdb_id`.
    normalized_name: String,
    name: String,
    /// Decoration TMDB id (may be null). Even when present, only
    /// rendered as a poster after the joined collection is verified.
    tmdb_id: Option<i64>,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    new_count: i64,
    last_visited_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

/// Build the client-facing summary. Poster lookup is gated on the
/// matching collection being `tmdb_verified` — without that signal
/// we refuse to fetch TMDB metadata to avoid surfacing the wrong
/// show's poster.
async fn summarize(state: &AppState, row: &iris_db::follows::FollowRow) -> FollowSummary {
    let trusted_tmdb = trusted_tmdb_id(state.db(), &row.normalized_name).await;
    // `series_follows` is TV-only — hint the namespace so a numerical
    // id collision with a movie can't serve a stranger's poster.
    let (poster_path, backdrop_path) = match (state.tmdb(), trusted_tmdb) {
        (Some(client), Some(tid)) => {
            // tid is a positive i64 from the DB; u64 conversion is safe.
            #[allow(clippy::cast_sign_loss)]
            let meta = client
                .lookup_with_kind(tid as u64, Some(crate::tmdb::TmdbKind::Tv))
                .await;
            meta.map_or((None, None), |m| (m.poster_path, m.backdrop_path))
        }
        _ => (None, None),
    };
    let new_count = iris_db::available_episodes::count_new_for_series(
        state.db(),
        &row.normalized_name,
        row.last_visited_at,
    )
    .await
    .unwrap_or(0);
    FollowSummary {
        id: row.id,
        normalized_name: row.normalized_name.clone(),
        name: row.name.clone(),
        tmdb_id: row.tmdb_id,
        poster_path,
        backdrop_path,
        new_count,
        last_visited_at: row.last_visited_at,
        created_at: row.created_at,
    }
}

/// Returns a TMDB id we trust enough to use for poster lookup —
/// i.e., one stored on a collection whose `tmdb_id` was written by
/// the post-verify enrichment path (which only fires when the
/// runtime probe matched). Returns None when no verified
/// collection joins to this normalised name.
async fn trusted_tmdb_id(
    pool: &iris_db::SqlitePool,
    normalized_name: &str,
) -> Option<i64> {
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT tmdb_id FROM collections \
         WHERE parsed_title_normalized = ?1 AND kind = 'tv' AND tmdb_id IS NOT NULL \
         ORDER BY created_at LIMIT 1",
    )
    .bind(normalized_name)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    row.map(|(t,)| t)
}

// ---------------------------------------------------------------------------
// DELETE /api/me/follows/:id
// ---------------------------------------------------------------------------

async fn remove(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let removed = iris_db::follows::delete(state.db(), user.id, id).await?;
    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

// ---------------------------------------------------------------------------
// GET /api/me/follows/:id/episodes
// ---------------------------------------------------------------------------
//
// SCENE-only: the canonical episode list is the union of
//   * episode_files (on disk)  — keyed on collection_id, join via
//     collections.parsed_title_normalized = follow.normalized_name
//   * available_episodes (indexer cache) — keyed on normalized_name
// Visiting bumps last_visited_at to clear the "X nouveaux" badge.

#[derive(Debug, Deserialize, Default)]
struct EpisodesQuery {
    /// Optional season filter — when set, only that season's rows
    /// are returned. Otherwise everything we know about ships in
    /// one response (covers the grouped Series page render).
    season: Option<u32>,
}

async fn episodes(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<EpisodesQuery>,
) -> ApiResult<Json<EpisodesResponse>> {
    // Dual resolution: old APK clients pass `series_follows.id`,
    // new clients pass `collection.id`. Both shapes resolve to the
    // same SCENE-normalised name, which is the actual join key.
    let identity = resolve_followish(&state, user.id, id)
        .await
        .ok_or(ApiError::NotFound)?;

    // 1. Files on disk — per-collection join via normalised name.
    let downloaded = iris_db::episode_files::list_for_normalized(
        state.db(),
        &identity.normalized_name,
    )
    .await
    .unwrap_or_default();

    // 2. Indexer-cached availability.
    let available = iris_db::available_episodes::list_best_for_series(
        state.db(),
        &identity.normalized_name,
    )
    .await
    .unwrap_or_default();

    // Merge: anything in `downloaded` wins; otherwise fall back to
    // the indexer hint. The two tables can overlap (we ingested an
    // episode that the indexer also still lists) — downloaded
    // status is the higher-signal answer.
    let mut by_key: BTreeMap<(i64, i64), EpisodeItem> = BTreeMap::new();

    for d in &downloaded {
        if let Some(s) = q.season {
            if d.season != i64::from(s) {
                continue;
            }
        }
        let watched = iris_db::playback::get(state.db(), user.id, &d.infohash, d.file_idx)
            .await
            .unwrap_or(None)
            .is_some_and(|p| p.completed);
        by_key.insert(
            (d.season, d.episode),
            EpisodeItem {
                season: d.season,
                episode: d.episode,
                status: EpisodeStatus::Downloaded,
                watched,
                infohash: Some(d.infohash.clone()),
                file_idx: Some(d.file_idx),
                indexer_provider: None,
                indexer_torrent_id: None,
                quality: None,
                seeders: None,
            },
        );
    }
    for a in &available {
        if let Some(s) = q.season {
            if a.season != i64::from(s) {
                continue;
            }
        }
        by_key.entry((a.season, a.episode)).or_insert(EpisodeItem {
            season: a.season,
            episode: a.episode,
            status: EpisodeStatus::Available,
            watched: false,
            infohash: None,
            file_idx: None,
            indexer_provider: Some(a.indexer_provider.clone()),
            indexer_torrent_id: Some(a.indexer_torrent_id.clone()),
            quality: a.quality.clone(),
            seeders: a.seeders,
        });
    }

    let items: Vec<EpisodeItem> = by_key.into_values().collect();

    // Bump visited timestamp AFTER reading — we don't need the
    // previous value past this point.
    match identity.source {
        FollowishSource::SeriesFollow => {
            let _ = iris_db::follows::mark_visited(state.db(), user.id, id).await;
        }
        FollowishSource::Collection => {
            let _ = iris_db::collections::touch_visited(state.db(), id).await;
        }
    }

    Ok(Json(EpisodesResponse {
        season: q.season,
        items,
    }))
}

#[derive(Debug, Serialize)]
struct EpisodesResponse {
    /// Echoes the request filter — `null` when the caller asked for
    /// the full set.
    season: Option<u32>,
    items: Vec<EpisodeItem>,
}

#[derive(Debug, Serialize)]
struct EpisodeItem {
    season: i64,
    episode: i64,
    status: EpisodeStatus,
    watched: bool,
    infohash: Option<String>,
    file_idx: Option<i64>,
    indexer_provider: Option<String>,
    indexer_torrent_id: Option<String>,
    quality: Option<String>,
    seeders: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum EpisodeStatus {
    Downloaded,
    Available,
}

// ---------------------------------------------------------------------------
// GET /api/me/follows/episode-context?infohash=X&file_idx=N
// ---------------------------------------------------------------------------
//
// "Préparer le suivant ?" plumbing for the player. Returns the
// follow id (if any) plus the `(season, episode + 1)` if we know
// it from `available_episodes` or already have it on disk.

#[derive(Debug, Deserialize)]
struct EpisodeContextParams {
    infohash: String,
    file_idx: i64,
}

#[derive(Debug, Serialize)]
struct EpisodeContext {
    followed: bool,
    current: Option<EpisodePoint>,
    next: Option<EpisodePoint>,
    /// Previous episode (symmetric to [`Self::next`]). Powers the
    /// "‹ Prev" chip in the TV player so the user can step back
    /// without bouncing through the Series detail screen.
    #[serde(skip_serializing_if = "Option::is_none")]
    prev: Option<EpisodePoint>,
}

#[derive(Debug, Serialize)]
struct EpisodePoint {
    follow_id: Option<Uuid>,
    season: i64,
    episode: i64,
    status: EpisodeStatus,
    /// Physical location for `status == Downloaded`. Lets the TV
    /// player navigate directly to the next episode without a
    /// round-trip through `/grab`. `None` for `Available` (the file
    /// doesn't exist on disk yet).
    #[serde(skip_serializing_if = "Option::is_none")]
    infohash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_idx: Option<i64>,
}

async fn episode_context(
    State(state): State<AppState>,
    user: AuthUser,
    Query(p): Query<EpisodeContextParams>,
) -> ApiResult<Json<EpisodeContext>> {
    let Some(current_row) =
        iris_db::episode_files::find_by_file(state.db(), &p.infohash, p.file_idx).await?
    else {
        // Standalone or movie file — no episode taxonomy to chain
        // off. Still surface a same-torrent fallback so a season pack
        // with no SCENE-recognised filenames keeps the "next" button
        // working.
        let next = same_torrent_next(state.db(), &p.infohash, p.file_idx + 1).await?;
        let prev = if p.file_idx > 0 {
            same_torrent_next(state.db(), &p.infohash, p.file_idx - 1).await?
        } else {
            None
        };
        return Ok(Json(EpisodeContext {
            followed: false,
            current: None,
            next,
            prev,
        }));
    };
    let collection = iris_db::collections::get(state.db(), current_row.collection_id).await?;
    let normalized = collection
        .as_ref()
        .and_then(|c| c.parsed_title_normalized.as_deref());

    let follow = if let Some(n) = normalized {
        iris_db::follows::get_by_normalized(state.db(), user.id, n).await?
    } else {
        None
    };

    let current = EpisodePoint {
        follow_id: follow.as_ref().map(|f| f.id),
        season: current_row.season,
        episode: current_row.episode,
        status: EpisodeStatus::Downloaded,
        infohash: Some(current_row.infohash.clone()),
        file_idx: Some(current_row.file_idx),
    };

    // 1. Try the next episode within the same season.
    // 2. If that's nowhere (downloaded nor available), try season N+1
    //    episode 1 — covers a binge across a season finale.
    // 3. Finally, fall back to file_idx+1 in the same torrent so
    //    season packs with no follow / collection-context still
    //    surface a "next" button.
    let next_collection = if let Some(n) = normalized {
        let same_season = (current_row.season, current_row.episode + 1);
        let by_same_season = lookup_next_episode(state.db(), follow.as_ref(), n, same_season).await;
        if by_same_season.is_some() {
            by_same_season
        } else {
            let next_season = (current_row.season + 1, 1);
            lookup_next_episode(state.db(), follow.as_ref(), n, next_season).await
        }
    } else {
        None
    };
    let next = match next_collection {
        Some(ep) => Some(ep),
        None => same_torrent_next(state.db(), &p.infohash, p.file_idx + 1).await?,
    };

    // Symmetric previous-episode lookup. (S, E-1), then the last
    // episode of S-1, then same-torrent file_idx-1.
    let prev_collection = if let Some(n) = normalized {
        if current_row.episode > 1 {
            let same_season = (current_row.season, current_row.episode - 1);
            lookup_next_episode(state.db(), follow.as_ref(), n, same_season).await
        } else if current_row.season > 1 {
            // Find the highest-numbered episode of the previous
            // season so the chip can land the user there.
            let prev_season = current_row.season - 1;
            let last_ep = iris_db::episode_files::list_for_normalized(state.db(), n)
                .await
                .unwrap_or_default()
                .into_iter()
                .filter(|r| r.season == prev_season)
                .map(|r| r.episode)
                .max();
            match last_ep {
                Some(ep) => {
                    lookup_next_episode(state.db(), follow.as_ref(), n, (prev_season, ep)).await
                }
                None => None,
            }
        } else {
            None
        }
    } else {
        None
    };
    let prev = match prev_collection {
        Some(ep) => Some(ep),
        None if p.file_idx > 0 => {
            same_torrent_next(state.db(), &p.infohash, p.file_idx - 1).await?
        }
        None => None,
    };

    Ok(Json(EpisodeContext {
        followed: follow.is_some(),
        current: Some(current),
        next,
        prev,
    }))
}

/// Resolve `(season, episode)` for `normalized_name` to an
/// [`EpisodePoint`], preferring on-disk over indexer-cached. Returns
/// `None` when neither layer knows about it. `available` is only
/// surfaced when the user actually follows the series — without a
/// follow they have no `/grab` endpoint to call anyway.
async fn lookup_next_episode(
    pool: &iris_db::SqlitePool,
    follow: Option<&iris_db::follows::FollowRow>,
    normalized_name: &str,
    (season, episode): (i64, i64),
) -> Option<EpisodePoint> {
    let on_disk = iris_db::episode_files::list_for_normalized(pool, normalized_name)
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|r| r.season == season && r.episode == episode);
    if let Some(row) = on_disk {
        return Some(EpisodePoint {
            follow_id: follow.map(|f| f.id),
            season,
            episode,
            status: EpisodeStatus::Downloaded,
            infohash: Some(row.infohash),
            file_idx: Some(row.file_idx),
        });
    }
    let follow = follow?;
    let avail = iris_db::available_episodes::list_best_for_series(pool, normalized_name)
        .await
        .unwrap_or_default();
    if avail.iter().any(|a| a.season == season && a.episode == episode) {
        return Some(EpisodePoint {
            follow_id: Some(follow.id),
            season,
            episode,
            status: EpisodeStatus::Available,
            infohash: None,
            file_idx: None,
        });
    }
    None
}

/// Last-resort fallback: if there's a sibling file at `file_idx` on
/// the SAME infohash and it has an `episode_files` row (= SCENE
/// parsing recognised it as an episode), return it as a downloaded
/// next. Used when the collection / follow context comes up empty,
/// e.g. season packs that landed without a follow or movies in a
/// folder torrent.
async fn same_torrent_next(
    pool: &iris_db::SqlitePool,
    infohash: &str,
    file_idx: i64,
) -> Result<Option<EpisodePoint>, ApiError> {
    let Some(row) = iris_db::episode_files::find_by_file(pool, infohash, file_idx).await? else {
        return Ok(None);
    };
    Ok(Some(EpisodePoint {
        follow_id: None,
        season: row.season,
        episode: row.episode,
        status: EpisodeStatus::Downloaded,
        infohash: Some(row.infohash),
        file_idx: Some(row.file_idx),
    }))
}

// ---------------------------------------------------------------------------
// POST /api/me/follows/:id/episodes/:season/:episode/grab
// ---------------------------------------------------------------------------
//
// Routed by follow id (UUID). Idempotent — if the episode is
// already on disk we short-circuit through the existing
// `episode_files` row.

#[derive(Debug, Serialize)]
pub(crate) struct GrabResponse {
    pub infohash: String,
    pub file_idx: i64,
    pub already_grabbed: bool,
}

async fn grab_episode(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, season, episode)): Path<(Uuid, i64, i64)>,
) -> ApiResult<Json<GrabResponse>> {
    let identity = resolve_followish(&state, user.id, id)
        .await
        .ok_or(ApiError::NotFound)?;
    // Auto-continuation ("prepare next episode"): keep the series in
    // its established language instead of defaulting to English. We
    // grab in the dominant owned language, falling back gracefully so
    // the next episode still downloads if that language has no offer.
    let dominant = dominant_owned_language(&state, &identity.normalized_name).await;
    grab_episode_core(
        &state,
        GrabEpisodeRequest {
            user_id: user.id,
            normalized_name: &identity.normalized_name,
            display_title: &identity.display_name,
            tmdb_id: identity.tmdb_id,
            season,
            episode,
            language: continuation_pref(dominant),
        },
    )
    .await
    .map(Json)
}

/// Where a "follow-ish" id came from in the dual-resolver flow.
/// APK 0.3.1 round-trips `series_follows.id`; post-0.4 clients
/// round-trip `collections.id`. The handler logic is identical;
/// only the `last_visited_at` bump goes to a different table.
enum FollowishSource {
    SeriesFollow,
    Collection,
}

struct FollowishIdentity {
    normalized_name: String,
    display_name: String,
    tmdb_id: Option<i64>,
    source: FollowishSource,
}

/// Try to resolve an opaque id as either a legacy `series_follows`
/// row (APK 0.3.1) or a `collections` row (post-0.4 clients). The
/// caller never has to care which it was — both produce the same
/// SCENE identity that downstream logic actually keys on.
async fn resolve_followish(
    state: &AppState,
    user_id: iris_core::ids::UserId,
    id: Uuid,
) -> Option<FollowishIdentity> {
    if let Ok(Some(follow)) = iris_db::follows::get_by_id(state.db(), user_id, id).await {
        return Some(FollowishIdentity {
            normalized_name: follow.normalized_name,
            display_name: follow.name,
            tmdb_id: follow.tmdb_id,
            source: FollowishSource::SeriesFollow,
        });
    }
    if let Ok(Some(c)) = iris_db::collections::get(state.db(), id).await {
        if c.kind == "tv" {
            if let Some(norm) = c.parsed_title_normalized {
                return Some(FollowishIdentity {
                    normalized_name: norm,
                    display_name: c.display_title,
                    tmdb_id: c.tmdb_id,
                    source: FollowishSource::Collection,
                });
            }
        }
    }
    None
}

/// Shared "grab a specific episode" body. Wraps the look-on-disk →
/// look-in-available_episodes-cache → fall-back-to-live-indexer
/// flow + the ingest plumbing. Both the legacy follows endpoint and
/// the new collection-keyed endpoint (`POST /api/library/collections/
/// :id/grab/:s/:e`) call this — the only difference between callers
/// is how they resolve the SCENE identity (follow row vs collection
/// row).
///
/// `language_pref` gates the live-indexer fallback so an English
/// Seedpool release can't be auto-grabbed for a French collection.
/// Pass `Language::Unknown` to accept any language (first ingest,
/// or genuinely-undecided household).
/// Bundle of every input `grab_episode_core` needs. Lives as a
/// struct (rather than positional args) because the call gathers
/// inputs from a handful of unrelated sources — auth context, SCENE
/// identity resolved off either a follow or a collection, a TMDB
/// id for verify — and "seven args in a row" was a bug magnet at
/// the call site.
pub(crate) struct GrabEpisodeRequest<'a> {
    pub user_id: iris_core::ids::UserId,
    pub normalized_name: &'a str,
    pub display_title: &'a str,
    pub tmdb_id: Option<i64>,
    pub season: i64,
    pub episode: i64,
    /// How the grab path treats language — see [`LangSel`].
    pub language: LangSel,
}

/// Language selection strategy for a grab.
///
/// The three modes encode the three ways a grab is triggered, and they
/// behave differently so each one stays honest:
///
/// * [`LangSel::Any`] — no preference (first ingest of a series, or the
///   legacy APK 0.3.1 follows path with no picker). Take the best offer
///   regardless of language.
/// * [`LangSel::Exact`] — the user clicked a specific language badge in
///   the library. That language or nothing: we never silently hand back
///   a different language. The "Play existing" short-circuit only fires
///   on an owned file in *exactly* that language, and the offer/indexer
///   pick filters strictly. (Fixes: clicking FR opening the owned EN.)
/// * [`LangSel::Prefer`] — auto-continuation ("prepare next episode").
///   Try the languages in order (the series' established language
///   first), then fall back to anything available so the next episode
///   still downloads instead of dead-ending or grabbing EN blindly.
#[derive(Debug, Clone)]
pub(crate) enum LangSel {
    Any,
    Exact(Language),
    Prefer(Vec<Language>),
}

impl LangSel {
    /// Build from an explicit UI language badge: `Some("french")` ⇒
    /// strict `Exact(French)`, `None` ⇒ `Any`. Used by the library grab
    /// button so the chosen language is always honoured.
    pub(crate) fn from_badge(lang: Option<&str>) -> Self {
        match lang {
            Some(l) => LangSel::Exact(Language::parse_tag(l)),
            None => LangSel::Any,
        }
    }
}

/// Pick one item from `items` (each tagged with its language) per the
/// selection strategy. `items` should already be in preference order
/// for ties (e.g. seeders-desc) so "first match" is also "best match".
fn select_by_lang<T: Clone>(items: &[(Language, T)], sel: &LangSel) -> Option<T> {
    match sel {
        LangSel::Any => items.first().map(|(_, t)| t.clone()),
        LangSel::Exact(l) => items.iter().find(|(lang, _)| lang == l).map(|(_, t)| t.clone()),
        LangSel::Prefer(order) => order
            .iter()
            .find_map(|w| items.iter().find(|(lang, _)| lang == w))
            .or_else(|| items.first())
            .map(|(_, t)| t.clone()),
    }
}

/// Coarse language of an `available_episodes` offer row.
fn offer_language(r: &iris_db::available_episodes::AvailableEpisodeRow) -> Language {
    Language::parse_tag(r.language.as_deref().unwrap_or(""))
}

/// Resolve the language of an on-disk torrent: SCENE name first, the
/// source provider's `default_language` as fallback. Mirrors
/// `library::resolve_torrent_language` so the grab path, the collection
/// view and search all agree on a file's language.
async fn resolve_owned_language(state: &AppState, infohash: &str) -> Language {
    let Some(t) = iris_db::torrents::find_by_infohash(state.db(), infohash)
        .await
        .ok()
        .flatten()
    else {
        return Language::Unknown;
    };
    let detected = detect_language(&t.name);
    if detected != Language::Unknown {
        return detected;
    }
    t.source_provider
        .as_deref()
        .and_then(|p| state.providers().default_language(p))
        .map_or(Language::Unknown, Language::parse_tag)
}

/// The series' established language: the most-owned language across its
/// downloaded episodes. Drives "prepare next episode" so a French
/// series keeps going in French instead of defaulting to English.
/// Priority on a tie / no data follows the household rule "majority FR,
/// else EN, else MULTI".
async fn dominant_owned_language(state: &AppState, normalized_name: &str) -> Language {
    let files = iris_db::episode_files::list_for_normalized(state.db(), normalized_name)
        .await
        .unwrap_or_default();
    let (mut fr, mut en, mut multi) = (0u32, 0u32, 0u32);
    for f in &files {
        match resolve_owned_language(state, &f.infohash).await {
            Language::French => fr += 1,
            Language::English => en += 1,
            Language::Multi => multi += 1,
            Language::Unknown => {}
        }
    }
    // "Majority FR ⇒ FR, else EN, else MULTI": French only on a strict
    // plurality; otherwise English wins (including ties and the
    // nothing-owned default), with Multi as the last resort.
    if fr > en && fr > multi {
        Language::French
    } else if en >= multi {
        Language::English
    } else {
        Language::Multi
    }
}

/// Build the ordered language preference for an auto-continuation grab:
/// the series' established language first, then EN, MULTI, FR as
/// graceful fallbacks (deduped, first occurrence wins).
fn continuation_pref(dominant: Language) -> LangSel {
    let mut order = vec![dominant];
    for l in [Language::English, Language::Multi, Language::French] {
        if !order.contains(&l) {
            order.push(l);
        }
    }
    LangSel::Prefer(order)
}

pub(crate) async fn grab_episode_core(
    state: &AppState,
    req: GrabEpisodeRequest<'_>,
) -> ApiResult<GrabResponse> {
    let GrabEpisodeRequest {
        user_id,
        normalized_name,
        display_title,
        tmdb_id,
        season,
        episode,
        language,
    } = req;

    // Auto-track for this user. With a multi-family household
    // (~10 viewers, mixed taste) the Watchlist is per-user; the act
    // of grabbing an episode is the strongest "I want to keep
    // watching this" signal we have. Fire before the on-disk short-
    // circuit so a "grab the one I already have" click still tracks.
    // Idempotent — `iris_db::follows::add` is a no-op when
    // (user_id, normalized_name) already exists.
    let _ = iris_db::follows::add(
        state.db(),
        user_id,
        normalized_name,
        display_title,
        tmdb_id,
    )
    .await;

    // Short-circuit only when we already hold the episode in the
    // requested language. An explicit FR badge click must NOT return
    // the owned EN file — `find_episode_file` honours `language` so the
    // grab proceeds and fetches the FR release instead.
    if let Some(existing) =
        find_episode_file(state, normalized_name, season, episode, &language).await?
    {
        return Ok(GrabResponse {
            infohash: existing.infohash,
            file_idx: existing.file_idx,
            already_grabbed: true,
        });
    }

    // Resolution order:
    //   1. Cached singleton for the requested (S, E) honouring `language`.
    //   2. Cached season pack covering the requested season. The
    //      pack ingest path runs to completion, then we SCENE-parse
    //      the resulting file list to extract the leaf matching
    //      (S, E) and return its (infohash, file_idx).
    //   3. Live indexer query (singleton-only — pack discovery
    //      lives on the periodic scheduler, not on the grab path).
    let pack_pick = if best_available(state.db(), normalized_name, season, episode, &language)
        .await?
        .is_none()
    {
        find_pack_offer(state.db(), normalized_name, season, &language).await?
    } else {
        None
    };
    if let Some(pack) = pack_pick {
        return ingest_pack_and_pick_episode(
            state,
            user_id,
            display_title,
            tmdb_id,
            pack,
            season,
            episode,
        )
        .await;
    }
    let pick = match best_available(state.db(), normalized_name, season, episode, &language).await?
    {
        Some(p) => p,
        None => find_via_indexer_for_identity(
            state,
            normalized_name,
            display_title,
            &language,
            season,
            episode,
        )
        .await?
        .ok_or(ApiError::NotFound)?,
    };

    let result = ingest_picked(
        state,
        &pick,
        ReprimeHint {
            display_title,
            season,
            episode: Some(episode),
        },
    )
    .await?;

    iris_db::torrents::upsert(
        state.db(),
        iris_db::torrents::NewTorrent {
            infohash: result.snapshot.infohash.clone(),
            name: result
                .snapshot
                .name
                .clone()
                .unwrap_or_else(|| format!("{display_title} S{season:02}E{episode:02}")),
            total_size_bytes: result.snapshot.total_size_bytes,
            source_provider: Some(pick.indexer_provider.clone()),
            source_external_id: Some(pick.indexer_torrent_id.clone()),
            // Carry the resolved tmdb_id through to the torrent so
            // the runtime probe can attempt a verify match. Stays
            // unverified until the probe confirms.
            tmdb_id,
            added_by: user_id,
        },
    )
    .await?;

    let file_idx = pick_largest_video_file(&result.snapshot.files);
    finalise_grabbed_episode(state.clone(), &result, tmdb_id, season, episode, file_idx).await?;

    Ok(GrabResponse {
        infohash: result.snapshot.infohash,
        file_idx,
        already_grabbed: result.already_managed,
    })
}

fn pick_largest_video_file(files: &[iris_torrent::FileEntry]) -> i64 {
    const VIDEO_EXTS: [&str; 10] = [
        "mkv", "mp4", "webm", "m4v", "avi", "mov", "ts", "mts", "m2ts", "wmv",
    ];
    files
        .iter()
        .filter(|f| {
            std::path::Path::new(&f.path)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| VIDEO_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        })
        .max_by_key(|f| f.size_bytes)
        .map_or(0, |f| f.index as i64)
}

/// Run the post-ingest plumbing: collection assignment (synchronous —
/// the Series page relies on the back-reference), then the
/// `episode_files` upsert keyed on the just-attached collection.
async fn finalise_grabbed_episode(
    state: AppState,
    result: &iris_torrent::IngestResult,
    tmdb_id: Option<i64>,
    season: i64,
    episode: i64,
    file_idx: i64,
) -> ApiResult<()> {
    let files: Vec<(usize, String)> = result
        .snapshot
        .files
        .iter()
        .map(|f| (f.index, f.path.clone()))
        .collect();
    crate::collection_assign::assign_after_ingest(
        state.db(),
        crate::collection_assign::EnrichDeps {
            tmdb: state.tmdb(),
            anilist: state.anilist(),
            providers: Some(state.providers()),
        },
        &result.snapshot.infohash,
        &result.snapshot.name.clone().unwrap_or_default(),
        tmdb_id,
        &files,
    )
    .await;

    if let Some(t) =
        iris_db::torrents::find_by_infohash(state.db(), &result.snapshot.infohash).await?
    {
        if let Some(collection_id) = t.collection_id {
            // Mirror the parser's SxxExx absolute rule: a fleuve grab
            // arrives as `season=1, episode=<absolute>`, so a high
            // episode under season 1 carries the absolute number.
            let absolute_episode = (season == 1
                && episode > i64::from(iris_media::filename::ABSOLUTE_EPISODE_THRESHOLD))
                .then_some(episode);
            let _ = iris_db::episode_files::upsert(
                state.db(),
                iris_db::episode_files::UpsertEpisodeFile {
                    collection_id,
                    season,
                    episode,
                    infohash: result.snapshot.infohash.clone(),
                    file_idx,
                    derived_from: iris_db::episode_files::DerivedFrom::TmdbMatch,
                    absolute_episode,
                },
            )
            .await;
        }
    }
    Ok(())
}

/// Look for the requested episode already on disk, honouring the
/// language selection. `LangSel::Exact(FR)` matches only an owned FR
/// file — so an explicit FR grab never short-circuits to the owned EN
/// (the user's "clicking FR opens English" bug). `Any` keeps the legacy
/// "first file wins" behaviour and skips the per-file language probe.
async fn find_episode_file(
    state: &AppState,
    normalized_name: &str,
    season: i64,
    episode: i64,
    sel: &LangSel,
) -> Result<Option<iris_db::episode_files::EpisodeFileRow>, sqlx::Error> {
    let rows = iris_db::episode_files::list_for_normalized(state.db(), normalized_name).await?;
    let candidates: Vec<_> = rows
        .into_iter()
        .filter(|r| r.season == season && r.episode == episode)
        .collect();
    if matches!(sel, LangSel::Any) {
        return Ok(candidates.into_iter().next());
    }
    let mut tagged = Vec::with_capacity(candidates.len());
    for c in candidates {
        let lang = resolve_owned_language(state, &c.infohash).await;
        tagged.push((lang, c));
    }
    Ok(select_by_lang(&tagged, sel))
}

struct PickedAvailability {
    magnet: String,
    indexer_provider: String,
    indexer_torrent_id: String,
    /// Pre-signed `.torrent` URL persisted on `available_episodes`
    /// at scan time. When set, the grab path fetches the bytes
    /// directly and skips `provider.resolve()` entirely — that's
    /// the in-memory-cache fallback path that 500s after restarts
    /// for Torznab / UNIT3D providers.
    download_url: Option<String>,
}

/// Context the grab path has handy that `provider.resolve()` doesn't —
/// pass it down so we can re-prime an empty link cache by doing a
/// targeted search. Restart-survivor mechanism #2 (the persisted
/// `download_url` is #1; this is what kicks in when the URL is null
/// — old DB rows pre-migration 0018 — or when the URL has 404'd).
struct ReprimeHint<'a> {
    /// Display title for the SCENE-form search query.
    display_title: &'a str,
    season: i64,
    /// `None` for season-pack grabs ("Show.Name.S01"); `Some` for
    /// individual-episode grabs.
    episode: Option<i64>,
}

async fn best_available(
    pool: &iris_db::SqlitePool,
    normalized_name: &str,
    season: i64,
    episode: i64,
    sel: &LangSel,
) -> Result<Option<PickedAvailability>, sqlx::Error> {
    let rows = iris_db::available_episodes::list_best_for_series(pool, normalized_name).await?;
    // `list_best_for_series` is best-by-seeders per (S, E, language) and
    // already ordered, so "first match" is also "best match". `Exact`
    // never substitutes another language (a clicked FR badge that has
    // no FR offer 404s rather than grabbing EN); `Prefer` walks the
    // continuation order and finally accepts anything available.
    let tagged: Vec<(Language, iris_db::available_episodes::AvailableEpisodeRow)> = rows
        .into_iter()
        .filter(|r| r.season == season && r.episode == episode)
        .map(|r| (offer_language(&r), r))
        .collect();
    Ok(select_by_lang(&tagged, sel).map(|r| PickedAvailability {
        magnet: r.magnet,
        indexer_provider: r.indexer_provider,
        indexer_torrent_id: r.indexer_torrent_id,
        download_url: r.download_url,
    }))
}

/// Resolve a `PickedAvailability` (magnet or provider-hosted
/// `.torrent`) through librqbit and return the runtime
/// [`IngestResult`]. Shared between the singleton grab and the
/// season-pack grab — both paths go through identical engine
/// plumbing, only the post-ingest "which file is the user's pick"
/// resolution differs.
async fn ingest_picked(
    state: &AppState,
    pick: &PickedAvailability,
    reprime: ReprimeHint<'_>,
) -> ApiResult<iris_torrent::IngestResult> {
    // Resolution order:
    //   1. Magnet — pre-resolved magnet URI, hand straight to librqbit.
    //   2. Persisted download_url — scheduler stashed a pre-signed
    //      `.torrent` URL at scan time. Fetch via the provider's
    //      `fetch_bytes` and feed bytes to the engine. Bypasses
    //      `provider.resolve()`'s in-memory link cache, which
    //      evaporates on restart and caused real 500s when c411 /
    //      UNIT3D dropped older releases off the search top page.
    //   3. provider.resolve() — last resort: per-id round-trip to
    //      the indexer. Works for torr9-style providers that don't
    //      ship a URL in the search payload, and as a fallback when
    //      the persisted URL has expired.
    if !pick.magnet.is_empty() {
        return state
            .engine()
            .add_from_magnet(&pick.magnet)
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("engine: {e}")));
    }
    if let Some(url) = pick.download_url.as_deref() {
        if let Some(provider) = state.providers().get(&pick.indexer_provider) {
            match provider.fetch_bytes(url).await {
                Ok(bytes) => {
                    return state
                        .engine()
                        .add_from_bytes(bytes.to_vec())
                        .await
                        .map_err(|e| ApiError::Internal(anyhow::anyhow!("engine: {e}")));
                }
                Err(e) => {
                    tracing::warn!(
                        url,
                        provider = %pick.indexer_provider,
                        error = %e,
                        "persisted download_url fetch failed; falling back to provider.resolve()",
                    );
                }
            }
        }
    }
    let provider = state.providers().get(&pick.indexer_provider).ok_or_else(|| {
        ApiError::Internal(anyhow::anyhow!(
            "provider `{}` no longer registered",
            pick.indexer_provider
        ))
    })?;
    // First resolve attempt — works when the provider's in-memory
    // link cache is still hot (i.e. the scheduler has touched this
    // (S, E) recently). On a fresh server boot the cache is empty
    // and Torznab / UNIT3D providers refuse the lookup.
    let source = match provider.resolve(&pick.indexer_torrent_id).await {
        Ok(s) => s,
        Err(first_err) => {
            // Re-prime: do a targeted search that, by side effect,
            // refills the provider's link cache for this exact
            // release. We have the show title + S/E because the
            // grab path knows them — `resolve()` doesn't get them
            // through the trait. After the re-prime, the second
            // `resolve()` lookup hits the cache.
            tracing::info!(
                provider = %pick.indexer_provider,
                external_id = %pick.indexer_torrent_id,
                reason = %first_err,
                "resolve cache miss — repriming via search and retrying",
            );
            let prime_q = build_reprime_query(&reprime);
            let _ = provider.search(&prime_q).await;
            provider
                .resolve(&pick.indexer_torrent_id)
                .await
                .map_err(|e| {
                    ApiError::Internal(anyhow::anyhow!(
                        "resolve after re-prime: {e} (first attempt: {first_err})"
                    ))
                })?
        }
    };
    match source {
        iris_core::search::TorrentSource::Magnet(m) => state.engine().add_from_magnet(&m).await,
        iris_core::search::TorrentSource::TorrentFile(b) => state.engine().add_from_bytes(b).await,
    }
    .map_err(|e| ApiError::Internal(anyhow::anyhow!("engine: {e}")))
}

/// Build the targeted `SearchQuery` used to re-prime a provider's
/// link cache after a `resolve()` miss. The episode field decides
/// pack vs singleton: `Some` produces `Show.Name S04E11`, `None`
/// produces `Show.Name S04` so a season-pack grab finds its
/// pack-shaped offer.
fn build_reprime_query(reprime: &ReprimeHint<'_>) -> iris_core::search::SearchQuery {
    use iris_core::search::{SearchQuery, SortField, SortOrder};
    let q = match reprime.episode {
        Some(e) => format!(
            "{title} S{s:02}E{e:02}",
            title = reprime.display_title,
            s = reprime.season,
            e = e,
        ),
        None => format!(
            "{title} S{s:02}",
            title = reprime.display_title,
            s = reprime.season,
        ),
    };
    SearchQuery {
        q,
        page: Some(1),
        limit: Some(20),
        sort_by: Some(SortField::Seeders),
        order: Some(SortOrder::Desc),
        kind: None,
        parsed_title: Some(iris_media::filename::series_key(reprime.display_title)),
        season: Some(reprime.season as u32),
        episode: reprime.episode.map(|e| e as u32),
        year: None,
    }
}

/// Look up a cached season pack that can satisfy a (season, episode)
/// request when no singleton offer exists. Applies the same language
/// selection as the singleton path: `Exact` won't drop an "FR" click
/// into an English pack; `Prefer` walks the continuation order.
async fn find_pack_offer(
    pool: &iris_db::SqlitePool,
    normalized_name: &str,
    season: i64,
    sel: &LangSel,
) -> Result<Option<PickedAvailability>, sqlx::Error> {
    let packs = iris_db::available_episodes::list_season_packs_for_series(pool, normalized_name)
        .await?;
    let tagged: Vec<(Language, iris_db::available_episodes::AvailableEpisodeRow)> = packs
        .into_iter()
        .filter(|p| p.season == season)
        .map(|p| (offer_language(&p), p))
        .collect();
    Ok(select_by_lang(&tagged, sel).map(|p| PickedAvailability {
        magnet: p.magnet,
        indexer_provider: p.indexer_provider,
        indexer_torrent_id: p.indexer_torrent_id,
        download_url: p.download_url,
    }))
}

/// Ingest a season pack and resolve a specific (season, episode)
/// leaf inside it. Runs the same engine ingest path as the singleton
/// grab; the difference is post-ingest: instead of returning the
/// pack's "main video", we SCENE-parse every file and pick the one
/// whose `(season, episode)` matches the request. If the parser
/// can't find the requested episode inside the pack we surface a
/// `NotFound` — the user clicked a non-existent (S, E).
async fn ingest_pack_and_pick_episode(
    state: &AppState,
    user_id: iris_core::ids::UserId,
    display_title: &str,
    tmdb_id: Option<i64>,
    pack: PickedAvailability,
    season: i64,
    episode: i64,
) -> ApiResult<GrabResponse> {
    let result = ingest_picked(
        state,
        &pack,
        // Re-prime hint targets the season pack, not the individual
        // episode — a c411 search of "Show.Name S04" turns up the
        // pack entry, refreshing the link cache the resolver needs.
        ReprimeHint {
            display_title,
            season,
            episode: None,
        },
    )
    .await?;

    // Find the leaf matching the requested (S, E). SCENE-parse each
    // path; pick the one whose `(season, episode)` matches. The
    // engine's snapshot already orders files SCENE-aware, but we
    // can't trust position alone — a multi-disc pack might have
    // `Disc1/Show.S01E04.mkv` ahead of `Disc2/Show.S01E12.mkv`
    // alphabetically without that matching the requested episode.
    let file_idx = result
        .snapshot
        .files
        .iter()
        .find_map(|f| {
            let leaf = f.path.rsplit('/').next().unwrap_or(&f.path);
            let parsed = iris_media::filename::parse(leaf)?;
            let s = parsed.season?;
            let e = parsed.episode?;
            if i64::from(s) == season && i64::from(e) == episode {
                Some(f.index as i64)
            } else {
                None
            }
        })
        .ok_or_else(|| {
            ApiError::NotFound
        })?;

    iris_db::torrents::upsert(
        state.db(),
        iris_db::torrents::NewTorrent {
            infohash: result.snapshot.infohash.clone(),
            name: result
                .snapshot
                .name
                .clone()
                .unwrap_or_else(|| format!("{display_title} S{season:02} pack")),
            total_size_bytes: result.snapshot.total_size_bytes,
            source_provider: Some(pack.indexer_provider.clone()),
            source_external_id: Some(pack.indexer_torrent_id.clone()),
            tmdb_id,
            added_by: user_id,
        },
    )
    .await?;

    // Same finalisation as the singleton path — collection_assign
    // will SCENE-parse every file in the pack and create
    // episode_files for the FULL season in one shot, so subsequent
    // calls for sibling episodes short-circuit through the on-disk
    // check.
    finalise_grabbed_episode(state.clone(), &result, tmdb_id, season, episode, file_idx).await?;

    Ok(GrabResponse {
        infohash: result.snapshot.infohash,
        file_idx,
        already_grabbed: result.already_managed,
    })
}

async fn find_via_indexer_for_identity(
    state: &AppState,
    normalized_name: &str,
    display_title: &str,
    sel: &LangSel,
    season: i64,
    episode: i64,
) -> Result<Option<PickedAvailability>, ApiError> {
    use iris_core::search::{SearchQuery, SortField, SortOrder};
    let query = SearchQuery {
        q: format!("{display_title} S{season:02}E{episode:02}"),
        page: Some(1),
        limit: Some(20),
        sort_by: Some(SortField::Seeders),
        order: Some(SortOrder::Desc),
        kind: None,
        // Targeted single-episode grab — hand providers the structured
        // hint so Torznab can dispatch as `t=tvsearch&season=&ep=`
        // instead of relying on substring matching alone.
        parsed_title: Some(iris_media::filename::series_key(display_title)),
        season: Some(season as u32),
        episode: Some(episode as u32),
        year: None,
    };
    let agg = state.providers().search_all(&query).await;
    // Apply the same language selection as the cached paths. Sort by
    // seeders first so "first match per language" is the best one;
    // `Exact` keeps only the requested language (404 if none — a
    // clicked FR badge mustn't pull EN), `Prefer` walks the
    // continuation order then accepts the top result, `Any` takes the
    // most-seeded.
    let mut results = agg.results;
    // Dead-torrent guard: never offer a confirmed 0-seeder release (its pieces
    // would never fully assemble). Unknown / ≥1 seeders pass through.
    results.retain(|r| r.seeders != Some(0));
    results.sort_by_key(|r| std::cmp::Reverse(r.seeders.unwrap_or(0)));
    let tagged: Vec<(Language, iris_core::search::SearchResult)> = results
        .into_iter()
        .map(|r| (detect_language(&r.title), r))
        .collect();
    let Some(best) = select_by_lang(&tagged, sel) else {
        return Ok(None);
    };
    let lang = detect_language(&best.title);
    let absolute_episode = iris_media::filename::parse(&best.title)
        .as_ref()
        .and_then(iris_media::filename::absolute_from_parsed)
        .map(i64::from);
    let _ = iris_db::available_episodes::upsert(
        state.db(),
        iris_db::available_episodes::UpsertAvailableEpisode {
            normalized_name: normalized_name.to_string(),
            season,
            episode,
            indexer_provider: best.provider_id.clone(),
            indexer_torrent_id: best.external_id.clone(),
            magnet: String::new(),
            quality: None,
            seeders: best.seeders.map(i64::from),
            size_bytes: best.size_bytes.map(|s| s as i64),
            language: Some(lang.as_str().to_string()),
            download_url: best.download_url.clone(),
            absolute_episode,
        },
    )
    .await;
    Ok(Some(PickedAvailability {
        magnet: String::new(),
        indexer_provider: best.provider_id,
        indexer_torrent_id: best.external_id,
        download_url: best.download_url,
    }))
}
