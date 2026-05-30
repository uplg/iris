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

use chrono::{Datelike, Utc};
use iris_core::ids::UserId;
use iris_db::SqlitePool;
use iris_db::catalog::{CatalogItem, CatalogOrder, CatalogQuery, WatchedSignal};
use serde::Serialize;
use uuid::Uuid;

use crate::state::AppState;
use crate::tmdb::TmdbKind;

/// Per-user cache window for the home shelf.
const RECO_CACHE_TTL: Duration = Duration::from_secs(60);
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
    now_year: i32,
    /// ISO 639-1 language filter; empty = any.
    languages: &'a [String],
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
) -> Ctx<'a> {
    Ctx {
        pool,
        excluded: &data.excluded,
        dismissed: &data.dismissed,
        affinity: &data.affinity,
        now_year: Utc::now().year(),
        languages,
    }
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
    let ctx = ctx_of(pool, &data, &languages);

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
    let ctx = ctx_of(pool, &data, &languages);

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
    if let Some(s) = because_you_watched(&ctx, &mut shown, &data.signals).await? {
        shelves.push(s);
    }
    if data.prefs.include_anime {
        if let Some(s) = new_anime_shelf(&ctx, &mut shown).await? {
            shelves.push(s);
        }
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
            order: CatalogOrder::Popularity,
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
                    order: CatalogOrder::Popularity,
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
                order: CatalogOrder::ReleaseDate,
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

/// "Because you watched <most recent>" — similar by its primary genre.
async fn because_you_watched(
    ctx: &Ctx<'_>,
    shown: &mut HashSet<Uuid>,
    signals: &[WatchedSignal],
) -> Result<Option<Shelf>, sqlx::Error> {
    let Some(seed) = signals.first() else {
        return Ok(None);
    };
    let Some(primary_genre) = parse_genre_ids(&seed.genres).first().copied() else {
        return Ok(None);
    };
    let mut seed_excluded = ctx.excluded.clone();
    seed_excluded.insert(seed.tmdb_id);
    let seed_ctx = Ctx {
        excluded: &seed_excluded,
        ..*ctx
    };
    let cards = scored_shelf(
        &seed_ctx,
        shown,
        CatalogQuery {
            is_anime: Some(false),
            genre: Some(primary_genre),
            limit: FETCH_LIMIT,
            ..Default::default()
        },
    )
    .await?;
    Ok((!cards.is_empty()).then(|| Shelf {
        key: format!("because_you_watched:{}", seed.tmdb_id),
        title: format!("Because you watched {}", seed.title),
        kind: None,
        items: cards,
    }))
}

/// New anime, most-recently-added (not language-filtered — anime is VO).
async fn new_anime_shelf(ctx: &Ctx<'_>, shown: &mut HashSet<Uuid>) -> Result<Option<Shelf>, sqlx::Error> {
    let cards = collect_shelf(
        ctx,
        shown,
        CatalogQuery {
            is_anime: Some(true),
            order: CatalogOrder::ReleaseDate,
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
    let rows = iris_db::catalog::query_for_user(ctx.pool, &q).await?;
    let cards: Vec<CatalogCard> = rows
        .into_iter()
        .filter(|r| !ctx.dismissed.contains(&r.id) && !shown.contains(&r.id))
        .filter(|r| r.tmdb_id.is_none_or(|id| !ctx.excluded.contains(&id)))
        .map(card_from_row)
        .take(SHELF_LIMIT)
        .collect();
    for c in &cards {
        shown.insert(c.catalog_id);
    }
    Ok(cards)
}

/// Like [`collect_shelf`] but re-ranks by the blended recommendation
/// score (popularity · recency · genre affinity).
async fn scored_shelf(
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

/// Rank candidates by the blended score, descending. Normalisation is
/// over the supplied set, so it's relative within a shelf.
fn score_and_sort(rows: Vec<CatalogItem>, ctx: &Ctx<'_>) -> Vec<CatalogItem> {
    let max_pop = rows.iter().filter_map(|r| r.popularity).fold(0.0_f64, f64::max);
    let max_aff = rows
        .iter()
        .map(|r| item_affinity(&r.genres, ctx.affinity))
        .fold(0.0_f64, f64::max);
    let mut scored: Vec<(f64, CatalogItem)> = rows
        .into_iter()
        .map(|r| {
            let s = blended_score(&r, max_pop, max_aff, ctx.affinity, ctx.now_year);
            (s, r)
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.into_iter().map(|(_, r)| r).collect()
}

/// Blended recommendation score: `0.4·popularity + 0.35·recency +
/// 0.25·affinity`, each normalised to 0..1 within the candidate set.
fn blended_score(
    item: &CatalogItem,
    max_pop: f64,
    max_aff: f64,
    affinity: &HashMap<i64, f64>,
    now_year: i32,
) -> f64 {
    let pop_n = if max_pop > 0.0 {
        item.popularity.unwrap_or(0.0) / max_pop
    } else {
        0.0
    };
    let year = item
        .release_date
        .as_deref()
        .and_then(|d| d.split('-').next())
        .and_then(|y| y.parse().ok());
    let rec = recency_score(year, now_year);
    let aff_n = if max_aff > 0.0 {
        item_affinity(&item.genres, affinity) / max_aff
    } else {
        0.0
    };
    0.4 * pop_n + 0.35 * rec + 0.25 * aff_n
}

/// 1.0 for the last year, decaying linearly to 0 at ~6 years old.
fn recency_score(year: Option<i32>, now_year: i32) -> f64 {
    let Some(y) = year else {
        return 0.0;
    };
    let age = now_year - y;
    if age <= 1 {
        1.0
    } else if age >= 6 {
        0.0
    } else {
        1.0 - f64::from(age - 1) / 5.0
    }
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
