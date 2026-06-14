//! Request-time recommendation assembly ("For You").
//!
//! Two surfaces, both fed from the shared `catalog_items` (populated by
//! the reco scheduler from TMDB + AniList):
//!
//! * **Home** — a single, long, *blended* shelf: the best of everything
//!   the user is into (movies + TV + anime), ranked by one score, with no
//!   per-genre splitting.
//! * **For-You page** — the same top picks plus organized sections
//!   (per-genre, "because you watched X", new anime).
//!
//! Owned / seen / dismissed titles are excluded everywhere, and a title
//! never repeats across the page's sections. Availability is NOT
//! pre-confirmed against trackers — the catalogue only holds titles that
//! are plausibly grabbable (movies gated to at-home releases at fetch
//! time), and the actual grab happens when the user clicks. Home is
//! cached per user for a short window.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Datelike, Utc};
use iris_core::ids::UserId;
use iris_db::SqlitePool;
use iris_db::catalog::{CatalogItem, CatalogOrder, CatalogQuery, WatchedSignal};
use serde::Serialize;
use uuid::Uuid;

use crate::state::AppState;
use crate::tmdb::{MediaMetadata, TmdbKind};

/// Per-user cache window for the home shelf.
const RECO_CACHE_TTL: Duration = Duration::from_mins(1);
/// Candidates fetched per kind to rank the blended feed.
const BLEND_WINDOW: i64 = 200;
/// SQL fetch cap for an organized section before exclusions.
const FETCH_LIMIT: i64 = 80;
/// Display cap per shelf/section, after exclusions.
const SHELF_LIMIT: usize = 30;
/// How many per-genre sections the For-You page surfaces.
const TOP_GENRE_SHELVES: usize = 3;
/// Affinity decays linearly to zero over this many days since last watch.
const AFFINITY_DECAY_DAYS: f64 = 30.0;
/// Weight a watched title contributes vs. an explicit onboarding pick.
const EXPLICIT_GENRE_WEIGHT: f64 = 2.0;

static RECO_CACHE: LazyLock<Mutex<HashMap<Uuid, (Instant, ForYou)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// A catalogue candidate as rendered on a shelf. Shape kept close to the
/// search / watchlist cards so the clients reuse the same card component.
#[derive(Debug, Clone, Serialize)]
pub struct CatalogCard {
    pub catalog_id: Uuid,
    pub tmdb_id: Option<i64>,
    pub kind: String,
    pub title: String,
    /// Fully-resolved poster URL. TMDB rows resolve their relative path to
    /// the image CDN; AniList-only rows pass their cover URL straight
    /// through — so clients render it directly without knowing the source.
    pub poster_url: Option<String>,
    /// Fully-resolved backdrop URL (wider; for a hero / preview).
    pub backdrop_url: Option<String>,
    pub overview: Option<String>,
    pub is_anime: bool,
    pub availability: String,
    /// Seeder count of the recorded release (rolling-window rows). `None` for
    /// lazy reco candidates and for torr9 RSS rows (re-checked at grab).
    pub seeders: Option<i64>,
    /// The recorded release's provider + id — enough for the client to open
    /// the same preview dialog as a search hit. `None` for lazy reco
    /// candidates (no resolved release yet → the client falls back to search).
    pub provider_id: Option<String>,
    pub external_id: Option<String>,
    pub year: Option<i32>,
    pub already_in_library: bool,
    pub library_infohash: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Shelf {
    /// Stable key for client routing (e.g. `for_you`, `genre:18`).
    pub key: String,
    pub title: String,
    /// Optional `"movie"`/`"tv"` hint when a shelf is single-kind.
    pub kind: Option<String>,
    pub items: Vec<CatalogCard>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForYou {
    pub shelves: Vec<Shelf>,
}

/// Shared per-request context threaded through the shelf builders. All
/// fields are cheap to copy (refs + an int), keeping signatures small.
#[derive(Clone, Copy)]
struct Ctx<'a> {
    pool: &'a SqlitePool,
    /// `tmdb_id`s the household owns or this user has seen.
    excluded: &'a HashSet<i64>,
    /// catalogue row ids this user dismissed.
    dismissed: &'a HashSet<Uuid>,
    /// genre id → weight.
    affinity: &'a HashMap<i64, f64>,
    /// ISO 639-1 language filter; empty = any.
    languages: &'a [String],
    /// Movies whose content year is below this are excluded from the discovery
    /// shelves (very old films freshly re-uploaded). TV is exempt.
    movie_cutoff_year: i32,
}

/// Map a user's language preference token to a TMDB ISO 639-1 code. Kept
/// here so the scheduler (which fetches per language) and the request
/// path (which filters per language) share one mapping.
pub(crate) fn pref_to_iso639(pref: &str) -> Option<&'static str> {
    match pref {
        "french" => Some("fr"),
        "english" => Some("en"),
        _ => None,
    }
}

/// Per-user inputs shared by the home + page builders.
struct UserData {
    prefs: iris_db::preferences::UserPreferences,
    excluded: HashSet<i64>,
    dismissed: HashSet<Uuid>,
    signals: Vec<WatchedSignal>,
    affinity: HashMap<i64, f64>,
}

async fn load_user_data(pool: &SqlitePool, user_id: UserId) -> Result<UserData, sqlx::Error> {
    let prefs = iris_db::preferences::get(pool, user_id).await?;
    let mut excluded: HashSet<i64> = iris_db::torrents::library_tmdb_ids(pool)
        .await?
        .into_iter()
        .collect();
    excluded.extend(iris_db::playback::watched_tmdb_ids(pool, user_id).await?);
    let dismissed: HashSet<Uuid> = iris_db::reco_feedback::dismissed_ids(pool, user_id)
        .await?
        .into_iter()
        .collect();
    let signals = iris_db::catalog::watched_genre_signals(pool, user_id).await?;
    let affinity = compute_affinity(&prefs.genres, &signals);
    Ok(UserData {
        prefs,
        excluded,
        dismissed,
        signals,
        affinity,
    })
}

fn languages_of(prefs: &iris_db::preferences::UserPreferences) -> Vec<String> {
    prefs
        .languages
        .iter()
        .filter_map(|l| pref_to_iso639(l).map(str::to_string))
        .collect()
}

fn ctx_of<'a>(
    pool: &'a SqlitePool,
    data: &'a UserData,
    languages: &'a [String],
    movie_cutoff_year: i32,
) -> Ctx<'a> {
    Ctx {
        pool,
        excluded: &data.excluded,
        dismissed: &data.dismissed,
        affinity: &data.affinity,
        languages,
        movie_cutoff_year,
    }
}

/// The oldest content year a MOVIE may have to appear in discovery, from the
/// configured `discovery.max_content_age_years` (TV is never gated by this).
fn movie_cutoff_year(state: &AppState) -> i32 {
    Utc::now().year()
        - i32::try_from(state.cfg().discovery.max_content_age_years.max(0)).unwrap_or(0)
}

/// HOME — a single blended "For You" shelf. Cached per user.
pub async fn for_you(state: &AppState, user_id: UserId) -> Result<ForYou, sqlx::Error> {
    let uuid: Uuid = user_id.into();
    if let Some(hit) = cache_get(uuid) {
        return Ok(hit);
    }
    let pool = state.db();
    let data = load_user_data(pool, user_id).await?;
    let languages = languages_of(&data.prefs);
    let ctx = ctx_of(pool, &data, &languages, movie_cutoff_year(state));

    let mut shown = HashSet::new();
    let items = blended_feed(&ctx, data.prefs.include_anime, &mut shown).await?;
    let shelves = if items.is_empty() {
        Vec::new()
    } else {
        vec![Shelf {
            key: "for_you".to_string(),
            title: "For You".to_string(),
            kind: None,
            items,
        }]
    };

    let result = ForYou { shelves };
    cache_put(uuid, result.clone());
    Ok(result)
}

/// FOR-YOU PAGE — the blended top picks plus organized sections (per
/// affinity genre, "because you watched X", new anime). A title shown in
/// an earlier section never repeats in a later one.
pub async fn for_you_page(state: &AppState, user_id: UserId) -> Result<ForYou, sqlx::Error> {
    let pool = state.db();
    let data = load_user_data(pool, user_id).await?;
    let languages = languages_of(&data.prefs);
    let ctx = ctx_of(pool, &data, &languages, movie_cutoff_year(state));

    let mut shown = HashSet::new();
    let mut shelves = Vec::new();

    let top = blended_feed(&ctx, data.prefs.include_anime, &mut shown).await?;
    if !top.is_empty() {
        // Page header is already "For You" — name this section so it
        // doesn't echo the page title.
        shelves.push(Shelf {
            key: "for_you".to_string(),
            title: "Top picks".to_string(),
            kind: None,
            items: top,
        });
    }

    let genre_names = genre_name_map(state).await;
    shelves.extend(genre_shelves(&ctx, &mut shown, &genre_names).await?);
    if let Some(s) = because_you_watched(state, &ctx, &mut shown, &data.signals).await? {
        shelves.push(s);
    }
    if data.prefs.include_anime
        && let Some(s) = new_anime_shelf(&ctx, &mut shown).await?
    {
        shelves.push(s);
    }

    Ok(ForYou { shelves })
}

/// The blended ranking — every interest mixed into one score
/// (popularity · recency · genre affinity), across movies + TV (+ anime
/// when enabled). Caps to one shelf's worth and records what it showed.
async fn blended_feed(
    ctx: &Ctx<'_>,
    include_anime: bool,
    shown: &mut HashSet<Uuid>,
) -> Result<Vec<CatalogCard>, sqlx::Error> {
    let mut rows: Vec<CatalogItem> = iris_db::catalog::query_for_user(
        ctx.pool,
        &CatalogQuery {
            languages: ctx.languages.to_vec(),
            is_anime: Some(false),
            // Rolling window only: the discovery feed shows what a tracker
            // actually has right now. Freshest uploads first, then re-ranked.
            only_available: true,
            order: CatalogOrder::Released,
            limit: BLEND_WINDOW,
            ..Default::default()
        },
    )
    .await?;
    if include_anime {
        // Anime isn't language-filtered (it's in VO) — the gate is the
        // include_anime preference.
        rows.extend(
            iris_db::catalog::query_for_user(
                ctx.pool,
                &CatalogQuery {
                    is_anime: Some(true),
                    only_available: true,
                    order: CatalogOrder::Released,
                    limit: BLEND_WINDOW,
                    ..Default::default()
                },
            )
            .await?,
        );
    }
    rows.retain(|r| {
        !ctx.dismissed.contains(&r.id)
            && !shown.contains(&r.id)
            && r.tmdb_id.is_none_or(|id| !ctx.excluded.contains(&id))
    });
    let cards: Vec<CatalogCard> = score_and_sort(rows, ctx)
        .into_iter()
        .take(SHELF_LIMIT)
        .map(card_from_row)
        .collect();
    for c in &cards {
        shown.insert(c.catalog_id);
    }
    Ok(cards)
}

/// Top genre-affinity sections, blended-scored within each genre.
async fn genre_shelves(
    ctx: &Ctx<'_>,
    shown: &mut HashSet<Uuid>,
    genre_names: &HashMap<i64, String>,
) -> Result<Vec<Shelf>, sqlx::Error> {
    let mut out = Vec::new();
    for gid in top_genres(ctx.affinity, TOP_GENRE_SHELVES) {
        // Freshest first — a genre section is for discovering recent
        // releases, not re-surfacing all-time classics.
        let cards = collect_shelf(
            ctx,
            shown,
            CatalogQuery {
                languages: ctx.languages.to_vec(),
                is_anime: Some(false),
                genre: Some(gid),
                only_available: true,
                order: CatalogOrder::Released,
                limit: FETCH_LIMIT,
                ..Default::default()
            },
        )
        .await?;
        if !cards.is_empty() {
            let title = genre_names
                .get(&gid)
                .cloned()
                .unwrap_or_else(|| "Recommended".to_string());
            out.push(Shelf {
                key: format!("genre:{gid}"),
                title,
                kind: None,
                items: cards,
            });
        }
    }
    Ok(out)
}

/// "Because you watched <most recent>" — driven by TMDB's own
/// recommendations + similar lists, not a genre proxy. These embrace older
/// titles on purpose: an established title is a strong availability signal,
/// and the grab resolves it lazily at click time (with the dead-torrent
/// guard). Each candidate is reconciled to a catalogue row (created lazily,
/// `availability='unknown'`, if not already present) so the card has a stable
/// id for grab/dismiss and shows `available` when it's also in the window.
async fn because_you_watched(
    state: &AppState,
    ctx: &Ctx<'_>,
    shown: &mut HashSet<Uuid>,
    signals: &[WatchedSignal],
) -> Result<Option<Shelf>, sqlx::Error> {
    let Some(seed) = signals.first() else {
        return Ok(None);
    };
    let Some(tmdb) = state.tmdb() else {
        return Ok(None);
    };
    let Ok(seed_id) = u64::try_from(seed.tmdb_id) else {
        return Ok(None);
    };
    let kind = if seed.kind == "tv" {
        TmdbKind::Tv
    } else {
        TmdbKind::Movie
    };
    // Collaborative ("also liked") + content-based ("similar") candidates.
    let mut metas = tmdb.recommendations(kind, seed_id).await;
    metas.extend(tmdb.similar(kind, seed_id).await);

    // Content-age floor: "old = probably available" stays a feature, but a
    // very old film (e.g. 1972) is dropped from recommendations too (TV
    // exempt). The availability benefit still holds within the cap.
    let mut cards = Vec::new();
    let mut seen_ids: HashSet<i64> = HashSet::new();
    for m in metas {
        let Ok(tmdb_id) = i64::try_from(m.tmdb_id) else {
            continue;
        };
        if tmdb_id == seed.tmdb_id || !seen_ids.insert(tmdb_id) {
            continue;
        }
        if ctx.excluded.contains(&tmdb_id) {
            continue;
        }
        if m.kind == TmdbKind::Movie
            && m.year
                .and_then(|y| i32::try_from(y).ok())
                .is_some_and(|y| y < ctx.movie_cutoff_year)
        {
            continue;
        }
        let Some(row) = get_or_create_lazy(ctx.pool, &m, kind).await? else {
            continue;
        };
        if ctx.dismissed.contains(&row.id) || !shown.insert(row.id) {
            continue;
        }
        cards.push(card_from_row(row));
        if cards.len() >= SHELF_LIMIT {
            break;
        }
    }
    Ok((!cards.is_empty()).then(|| Shelf {
        key: format!("because_you_watched:{}", seed.tmdb_id),
        title: format!("Because you watched {}", seed.title),
        kind: None,
        items: cards,
    }))
}

/// Fetch the catalogue row for a recommendation candidate, creating a lazy
/// (`availability='unknown'`, no release facts) row from TMDB metadata when it
/// isn't in the catalogue yet. An existing row — whether a confirmed
/// rolling-window row or a prior lazy one — is returned untouched, so we never
/// clobber a tracker-confirmed availability with 'unknown'.
async fn get_or_create_lazy(
    pool: &SqlitePool,
    m: &MediaMetadata,
    kind: TmdbKind,
) -> Result<Option<CatalogItem>, sqlx::Error> {
    let Ok(tmdb_id) = i64::try_from(m.tmdb_id) else {
        return Ok(None);
    };
    if let Some(row) = iris_db::catalog::find_by_tmdb(pool, tmdb_id).await? {
        return Ok(Some(row));
    }
    let item = iris_db::catalog::NewCatalogItem {
        tmdb_id: Some(tmdb_id),
        anilist_id: None,
        kind: match kind {
            TmdbKind::Movie => "movie",
            TmdbKind::Tv => "tv",
        }
        .to_string(),
        title: m.title.clone(),
        original_language: m.original_language.clone(),
        genres: m.genre_ids.iter().map(|&g| i64::from(g)).collect(),
        is_anime: false,
        poster_path: m.poster_path.clone(),
        backdrop_path: m.backdrop_path.clone(),
        overview: m.overview.clone(),
        popularity: m.popularity,
        vote_average: m.vote_score,
        release_date: m.release_date.clone(),
        source: Some("reco:tmdb".to_string()),
        // Lazy candidate: availability resolved at click time (old = likely
        // available); no release facts until then.
        availability: "unknown".to_string(),
        seeders: None,
        provider_id: None,
        external_id: None,
        download_url: None,
        infohash: None,
        language: None,
        released_at: None,
    };
    iris_db::catalog::upsert_item(pool, &item).await?;
    iris_db::catalog::find_by_tmdb(pool, tmdb_id).await
}

/// New anime, most-recently-added (not language-filtered — anime is VO).
async fn new_anime_shelf(
    ctx: &Ctx<'_>,
    shown: &mut HashSet<Uuid>,
) -> Result<Option<Shelf>, sqlx::Error> {
    let cards = collect_shelf(
        ctx,
        shown,
        CatalogQuery {
            is_anime: Some(true),
            only_available: true,
            order: CatalogOrder::Released,
            limit: FETCH_LIMIT,
            ..Default::default()
        },
    )
    .await?;
    Ok((!cards.is_empty()).then(|| Shelf {
        key: "new_anime".to_string(),
        title: "New anime".to_string(),
        kind: None,
        items: cards,
    }))
}

/// Run a shelf query, drop owned/seen/dismissed/already-shown, map to
/// cards, cap to the display limit. Records what it shows.
async fn collect_shelf(
    ctx: &Ctx<'_>,
    shown: &mut HashSet<Uuid>,
    q: CatalogQuery,
) -> Result<Vec<CatalogCard>, sqlx::Error> {
    let rows: Vec<CatalogItem> = iris_db::catalog::query_for_user(ctx.pool, &q)
        .await?
        .into_iter()
        .filter(|r| !ctx.dismissed.contains(&r.id) && !shown.contains(&r.id))
        .filter(|r| r.tmdb_id.is_none_or(|id| !ctx.excluded.contains(&id)))
        .collect();
    // Rank by the fresh score (upload + content recency × affinity × pop) so
    // very-old movies sink even within a genre/anime shelf.
    let cards: Vec<CatalogCard> = score_and_sort(rows, ctx)
        .into_iter()
        .take(SHELF_LIMIT)
        .map(card_from_row)
        .collect();
    for c in &cards {
        shown.insert(c.catalog_id);
    }
    Ok(cards)
}

/// Rank rolling-window candidates by the fresh score, descending.
/// Normalisation is over the supplied set, so it's relative within a shelf.
fn score_and_sort(rows: Vec<CatalogItem>, ctx: &Ctx<'_>) -> Vec<CatalogItem> {
    // Hard content-age floor for discovery: drop very old movies — including
    // rows already in the catalogue from before the cap, so the effect is
    // immediate, not after the next GC. TV is exempt.
    let rows: Vec<CatalogItem> = rows
        .into_iter()
        .filter(|r| r.kind != "movie" || item_year(r).is_none_or(|y| y >= ctx.movie_cutoff_year))
        .collect();
    let max_pop = rows
        .iter()
        .filter_map(|r| r.popularity)
        .fold(0.0_f64, f64::max);
    let max_aff = rows
        .iter()
        .map(|r| item_affinity(&r.genres, ctx.affinity))
        .fold(0.0_f64, f64::max);
    let now = Utc::now();
    let mut scored: Vec<(f64, CatalogItem)> = rows
        .into_iter()
        .map(|r| {
            let s = fresh_score(&r, max_pop, max_aff, ctx.affinity, now);
            (s, r)
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.into_iter().map(|(_, r)| r).collect()
}

/// Rolling-window score: `0.4·upload-recency + 0.25·content-recency +
/// 0.2·affinity + 0.15·popularity`, each normalised to 0..1. Upload-freshness
/// (how recently the release dropped on a tracker) leads, but **content age**
/// now tempers it for movies — a 1972 film freshly re-uploaded sinks instead
/// of riding upload-recency to the top. TV is exempt (a long-running series
/// airing now isn't penalised for its first-air year).
fn fresh_score(
    item: &CatalogItem,
    max_pop: f64,
    max_aff: f64,
    affinity: &HashMap<i64, f64>,
    now: DateTime<Utc>,
) -> f64 {
    let pop_n = if max_pop > 0.0 {
        item.popularity.unwrap_or(0.0) / max_pop
    } else {
        0.0
    };
    let aff_n = if max_aff > 0.0 {
        item_affinity(&item.genres, affinity) / max_aff
    } else {
        0.0
    };
    let upload_rec = upload_recency(item.released_at, now);
    let content_rec = if item.kind == "movie" {
        content_recency(item_year(item), now.year())
    } else {
        1.0
    };
    0.4 * upload_rec + 0.25 * content_rec + 0.2 * aff_n + 0.15 * pop_n
}

/// 1.0 for a just-uploaded release, decaying linearly to 0 at ~4 weeks (the
/// retention window). A row with no upload time scores neutral.
fn upload_recency(released_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> f64 {
    let Some(up) = released_at else {
        return 0.5;
    };
    let days = (now - up).num_days().max(0);
    let days = f64::from(i32::try_from(days).unwrap_or(i32::MAX));
    (1.0 - days / 28.0).clamp(0.0, 1.0)
}

/// Content-age weight: 1.0 for the last ~2 years, decaying to a 0.1 floor by
/// ~20 years old (so a classic can still appear, just far down). Unknown year
/// scores neutral.
fn content_recency(year: Option<i32>, now_year: i32) -> f64 {
    let Some(y) = year else {
        return 0.5;
    };
    let age = (now_year - y).max(0);
    if age <= 2 {
        1.0
    } else if age >= 20 {
        0.1
    } else {
        1.0 - (f64::from(age - 2) / 18.0) * 0.9
    }
}

/// Parse the `YYYY` content year from a catalogue row's release date.
fn item_year(item: &CatalogItem) -> Option<i32> {
    item.release_date
        .as_deref()
        .and_then(|d| d.split('-').next())
        .and_then(|y| y.parse().ok())
}

/// Sum of the user's genre weights over an item's genres.
fn item_affinity(genres_json: &str, weights: &HashMap<i64, f64>) -> f64 {
    parse_genre_ids(genres_json)
        .iter()
        .filter_map(|g| weights.get(g))
        .sum()
}

fn parse_genre_ids(json: &str) -> Vec<i64> {
    serde_json::from_str(json).unwrap_or_default()
}

/// Genre weights: explicit onboarding picks (heavier) plus history, where
/// each watched title's genres decay linearly with time-since-watch.
fn compute_affinity(prefs_genres: &[i64], signals: &[WatchedSignal]) -> HashMap<i64, f64> {
    let mut weights: HashMap<i64, f64> = HashMap::new();
    for g in prefs_genres {
        *weights.entry(*g).or_insert(0.0) += EXPLICIT_GENRE_WEIGHT;
    }
    let now = Utc::now();
    for s in signals {
        let days = i32::try_from((now - s.watched_at).num_days().max(0)).unwrap_or(i32::MAX);
        let decay = (1.0 - f64::from(days) / AFFINITY_DECAY_DAYS).max(0.0);
        if decay <= 0.0 {
            continue;
        }
        for g in parse_genre_ids(&s.genres) {
            *weights.entry(g).or_insert(0.0) += decay;
        }
    }
    weights
}

/// The highest-weighted genre ids, descending.
fn top_genres(affinity: &HashMap<i64, f64>, n: usize) -> Vec<i64> {
    let mut ranked: Vec<(i64, f64)> = affinity
        .iter()
        .filter(|(_, w)| **w > 0.0)
        .map(|(g, w)| (*g, *w))
        .collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    ranked.into_iter().take(n).map(|(g, _)| g).collect()
}

/// Merged movie + TV genre id → name map, for section titles. Empty when
/// TMDB is unconfigured (sections fall back to a generic title).
async fn genre_name_map(state: &AppState) -> HashMap<i64, String> {
    let mut map = HashMap::new();
    if let Some(tmdb) = state.tmdb() {
        for kind in [TmdbKind::Movie, TmdbKind::Tv] {
            for g in tmdb.genre_list(kind).await {
                map.entry(i64::from(g.id)).or_insert(g.name);
            }
        }
    }
    map
}

/// Resolve a stored image path to a full URL at the given TMDB size. TMDB
/// rows store a relative path (`/abc.jpg`) → CDN URL; AniList rows store a
/// full URL → passed through untouched.
fn image_url(path: Option<&str>, size: &str) -> Option<String> {
    let p = path?;
    if p.is_empty() {
        None
    } else if p.starts_with("http") {
        Some(p.to_string())
    } else {
        Some(format!("https://image.tmdb.org/t/p/{size}{p}"))
    }
}

/// Drop a user's cached home shelf — called when prefs change or a card is
/// dismissed, so the next request rebuilds immediately.
pub(crate) fn invalidate(user_id: UserId) {
    let uuid: Uuid = user_id.into();
    if let Ok(mut guard) = RECO_CACHE.lock() {
        guard.remove(&uuid);
    }
}

fn card_from_row(row: CatalogItem) -> CatalogCard {
    let year = row
        .release_date
        .as_deref()
        .and_then(|d| d.split('-').next())
        .and_then(|y| y.parse().ok());
    CatalogCard {
        catalog_id: row.id,
        tmdb_id: row.tmdb_id,
        kind: row.kind,
        title: row.title,
        poster_url: image_url(row.poster_path.as_deref(), "w342"),
        backdrop_url: image_url(row.backdrop_path.as_deref(), "w780"),
        overview: row.overview,
        is_anime: row.is_anime,
        availability: row.availability,
        seeders: row.seeders,
        provider_id: row.provider_id,
        external_id: row.external_id,
        year,
        already_in_library: false,
        library_infohash: None,
    }
}

fn cache_get(user: Uuid) -> Option<ForYou> {
    let guard = RECO_CACHE.lock().ok()?;
    let (at, value) = guard.get(&user)?;
    if at.elapsed() < RECO_CACHE_TTL {
        Some(value.clone())
    } else {
        None
    }
}

fn cache_put(user: Uuid, value: ForYou) {
    if let Ok(mut guard) = RECO_CACHE.lock() {
        guard.insert(user, (Instant::now(), value));
    }
}
