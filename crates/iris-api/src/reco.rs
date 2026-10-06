//! Request-time discovery assembly: the "For You" shelves and the mood board.
//!
//! No recommender model. Every shelf is the outside world's pulse
//! (`pulse_signals`: TMDB / SIMKL trending, fresh digital releases, series on
//! the air, curated moods) intersected with what our trackers can serve
//! (`catalog_items`, tracker-confirmed only), then fitted to the account:
//!
//! * hidden: what this account watched or dismissed, anime unless opted in,
//!   releases in none of the account's languages;
//! * marked: titles already in the library (instant play);
//! * nudged: a transparent genre boost from the account's onboarding picks
//!   and its own watch history — never a reordering by other accounts.
//!
//! A title never repeats across one response's shelves. Cards are
//! title-references: they route to search, never to the recorded release.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use chrono::{Datelike, Utc};
use iris_core::ids::UserId;
use iris_core::search::MediaKind;
use iris_db::SqlitePool;
use iris_db::catalog::{CatalogItem, CatalogOrder, CatalogQuery};
use iris_db::pulse::Signal;
use iris_media::filename::Language;
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::pulse::{GenreRule, MOODS, Mood};
use crate::state::AppState;
use crate::tmdb::TmdbKind;

/// Per-user cache window for the shelves.
const CACHE_TTL: Duration = Duration::from_mins(1);
/// The whole tracker-confirmed catalogue is a few thousand rows: read it all.
const CATALOG_SNAPSHOT: i64 = 6000;
/// Display cap per shelf.
const SHELF_LIMIT: usize = 30;
/// "Hot on our trackers" looks at uploads this recent.
const HOT_WINDOW_DAYS: i64 = 7;
/// Max cards in a mood result.
const MOOD_LIMIT: usize = 40;
/// A mood needs this many grabbable titles to get a tile.
const MOOD_MIN: usize = 3;
/// Weight of one onboarding genre pick vs. one recent watch.
const EXPLICIT_GENRE_WEIGHT: f64 = 2.0;
/// Watch-history genre weight fades linearly to this floor over the window.
const HISTORY_DECAY_DAYS: f64 = 180.0;
const HISTORY_FLOOR: f64 = 0.25;
/// Most recent watched titles feeding the genre boost.
const HISTORY_TITLES: i64 = 40;
/// How far the genre boost can lift a score (×1 … ×1.5).
const AFFINITY_BOOST: f64 = 0.5;

type ShelfCache = HashMap<(Uuid, Surface), (Instant, ForYou)>;

static CACHE: LazyLock<Mutex<ShelfCache>> = LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Surface {
    Home,
    Page,
}

/// A catalogue candidate as rendered on a shelf. Shape kept close to the
/// search / watchlist cards so the clients reuse the same card component.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CatalogCard {
    pub catalog_id: Uuid,
    pub tmdb_id: Option<i64>,
    pub kind: MediaKind,
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
    /// Always `None`: a card names a title, not a specific release.
    pub seeders: Option<i64>,
    /// Always `None`, which routes web and TV to search for the title.
    pub provider_id: Option<String>,
    pub external_id: Option<String>,
    pub year: Option<i32>,
    pub already_in_library: bool,
    pub library_infohash: Option<String>,
    /// Why this card surfaced (e.g. "1.8k watching today"). Additive —
    /// clients that ignore it are unaffected.
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Shelf {
    /// Stable key for client routing (e.g. `trending`, `hot`).
    pub key: String,
    pub title: String,
    /// Optional `"movie"`/`"tv"` hint when a shelf is single-kind.
    pub kind: Option<String>,
    pub items: Vec<CatalogCard>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ForYou {
    pub shelves: Vec<Shelf>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MoodTile {
    /// Stable routing id (e.g. `chills`).
    pub id: String,
    /// Display label (`Chills`).
    pub label: String,
    /// Backdrop of the mood's top title, or `None` (client shows a gradient).
    pub backdrop_url: Option<String>,
    /// The mood's top title right now ("the chiller of the moment").
    /// Additive — older clients ignore it.
    pub featured_title: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MoodBoard {
    /// Curated moods with something grabbable, the account's genres first.
    pub moods: Vec<MoodTile>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MoodResults {
    pub mood: String,
    /// `"movie"` | `"tv"`.
    pub kind: String,
    pub items: Vec<CatalogCard>,
}

/// One account's lens on the shared catalogue.
struct Viewer {
    languages: Vec<Language>,
    include_anime: bool,
    watched: HashSet<i64>,
    dismissed: HashSet<Uuid>,
    library: HashSet<i64>,
    affinity: HashMap<i64, f64>,
    max_affinity: f64,
}

impl Viewer {
    async fn load(state: &AppState, user_id: UserId) -> Result<Self, sqlx::Error> {
        let pool = state.db();
        let prefs = iris_db::preferences::get(pool, user_id).await?;
        let watched = iris_db::playback::watched_tmdb_ids(pool, user_id)
            .await?
            .into_iter()
            .collect();
        let dismissed = iris_db::reco_feedback::dismissed_ids(pool, user_id)
            .await?
            .into_iter()
            .collect();
        let library = iris_db::torrents::library_tmdb_ids(pool)
            .await?
            .into_iter()
            .collect();
        let affinity = affinity(state, user_id, &prefs.genres).await?;
        let max_affinity = affinity.values().copied().fold(0.0, f64::max);
        Ok(Self {
            languages: prefs
                .languages
                .iter()
                .map(|l| Language::parse_tag(l))
                .filter(|l| *l != Language::Unknown)
                .collect(),
            include_anime: prefs.include_anime,
            watched,
            dismissed,
            library,
            affinity,
            max_affinity,
        })
    }

    /// Whether this account should see the row at all.
    fn admits(&self, r: &CatalogItem) -> bool {
        if self.dismissed.contains(&r.id) || r.tmdb_id.is_some_and(|id| self.watched.contains(&id))
        {
            return false;
        }
        if r.is_anime {
            // Anime is watched in VO: the opt-in is the language gate.
            return self.include_anime;
        }
        let release = r
            .language
            .as_deref()
            .map_or(Language::Unknown, Language::parse_tag);
        self.languages.is_empty() || self.languages.iter().any(|l| release.satisfies(*l))
    }

    /// 0..1 — how much of the account's taste the row's genres carry.
    fn taste(&self, r: &CatalogItem) -> f64 {
        if self.max_affinity <= 0.0 {
            return 0.0;
        }
        let sum: f64 = genre_ids(r)
            .iter()
            .filter_map(|g| self.affinity.get(g))
            .sum();
        (sum / self.max_affinity).min(1.0)
    }

    fn boost(&self, r: &CatalogItem, score: f64) -> f64 {
        score * (1.0 + AFFINITY_BOOST * self.taste(r))
    }

    fn card(&self, row: &CatalogItem, reason: Option<String>) -> CatalogCard {
        let in_library = row.tmdb_id.is_some_and(|id| self.library.contains(&id));
        CatalogCard {
            already_in_library: in_library,
            reason: if in_library {
                Some("In the library".to_string())
            } else {
                reason
            },
            ..card(row)
        }
    }
}

/// Genre weights: onboarding picks, plus the genres of the account's recent
/// watches (TMDB metadata, cached), fading with age but never to zero.
async fn affinity(
    state: &AppState,
    user_id: UserId,
    picks: &[i64],
) -> Result<HashMap<i64, f64>, sqlx::Error> {
    let mut weights: HashMap<i64, f64> = HashMap::new();
    for g in picks {
        *weights.entry(*g).or_default() += EXPLICIT_GENRE_WEIGHT;
    }
    let Some(tmdb) = state.tmdb() else {
        return Ok(weights);
    };
    let watched =
        iris_db::catalog::recent_watched_titles(state.db(), user_id, HISTORY_TITLES).await?;
    let lookups = watched.iter().filter_map(|w| {
        let id = u64::try_from(w.tmdb_id).ok()?;
        let kind = if w.kind == "tv" {
            TmdbKind::Tv
        } else {
            TmdbKind::Movie
        };
        Some(tmdb.lookup_with_kind(id, Some(kind)))
    });
    let metas = futures::future::join_all(lookups).await;
    let now = Utc::now();
    for (w, meta) in watched.iter().zip(metas) {
        let Some(meta) = meta else {
            continue;
        };
        let days =
            f64::from(i32::try_from((now - w.watched_at).num_days().max(0)).unwrap_or(i32::MAX));
        let weight = (1.0 - days / HISTORY_DECAY_DAYS).max(HISTORY_FLOOR);
        for g in meta.genre_ids {
            *weights.entry(i64::from(g)).or_default() += weight;
        }
    }
    Ok(weights)
}

/// The tracker-confirmed catalogue, keyed by `(tmdb_id, kind)`.
struct Catalog {
    rows: Vec<CatalogItem>,
    by_title: HashMap<(i64, String), usize>,
}

impl Catalog {
    async fn load(pool: &SqlitePool) -> Result<Self, sqlx::Error> {
        let rows = iris_db::catalog::query_for_user(
            pool,
            &CatalogQuery {
                only_available: true,
                order: CatalogOrder::Released,
                limit: CATALOG_SNAPSHOT,
                ..Default::default()
            },
        )
        .await?;
        let mut by_title = HashMap::new();
        for (i, r) in rows.iter().enumerate() {
            if let Some(id) = r.tmdb_id {
                by_title.entry((id, r.kind.clone())).or_insert(i);
            }
        }
        Ok(Self { rows, by_title })
    }

    fn get(&self, tmdb_id: i64, kind: &str) -> Option<&CatalogItem> {
        self.by_title
            .get(&(tmdb_id, kind.to_string()))
            .map(|&i| &self.rows[i])
    }

    fn max_seeders(&self) -> i64 {
        self.rows
            .iter()
            .filter_map(|r| r.seeders)
            .max()
            .unwrap_or(0)
    }
}

/// Builds one response's shelves, keeping titles unique across them.
struct Assembly<'a> {
    viewer: &'a Viewer,
    catalog: &'a Catalog,
    signals: &'a [Signal],
    shown: HashSet<Uuid>,
    max_seeders: i64,
}

impl<'a> Assembly<'a> {
    fn new(viewer: &'a Viewer, catalog: &'a Catalog, signals: &'a [Signal]) -> Self {
        Self {
            viewer,
            catalog,
            signals,
            shown: HashSet::new(),
            max_seeders: catalog.max_seeders(),
        }
    }

    fn seeders_score(&self, r: &CatalogItem) -> f64 {
        seeders_score(r.seeders, self.max_seeders)
    }

    /// Rank, dedup against earlier shelves, cap, and record what's shown.
    fn shelf(
        &mut self,
        key: &str,
        title: &str,
        kind: Option<&str>,
        mut scored: Vec<(f64, &'a CatalogItem, Option<String>)>,
    ) -> Option<Shelf> {
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut items = Vec::new();
        for (_, row, reason) in scored {
            if items.len() >= SHELF_LIMIT {
                break;
            }
            if self.viewer.admits(row) && self.shown.insert(row.id) {
                items.push(self.viewer.card(row, reason));
            }
        }
        (!items.is_empty()).then(|| Shelf {
            key: key.to_string(),
            title: title.to_string(),
            kind: kind.map(str::to_string),
            items,
        })
    }

    /// The listed titles of `list` we can serve, with a 0..1 list-position
    /// score (1 = top of its list for its kind).
    fn listed(&self, list: &str) -> Vec<(f64, &'a Signal, &'a CatalogItem)> {
        let mut lengths: HashMap<&str, i64> = HashMap::new();
        for s in self.signals.iter().filter(|s| s.list == list) {
            let n = lengths.entry(s.kind.as_str()).or_default();
            *n = (*n).max(s.rank + 1);
        }
        self.signals
            .iter()
            .filter(|s| s.list == list)
            .filter_map(|s| {
                let row = self.catalog.get(s.tmdb_id, &s.kind)?;
                let len = lengths.get(s.kind.as_str()).copied().unwrap_or(1).max(1);
                Some((rank_score(s.rank, len), s, row))
            })
            .collect()
    }

    /// "Trending now": TMDB's week and SIMKL's day, a title on both lifted.
    fn trending(&mut self) -> Option<Shelf> {
        let mut by_row: HashMap<Uuid, (f64, f64, Option<i64>, &'a CatalogItem)> = HashMap::new();
        for (score, _, row) in self.listed("trending") {
            by_row.entry(row.id).or_insert((0.0, 0.0, None, row)).0 = score;
        }
        for (score, s, row) in self.listed("simkl") {
            let e = by_row.entry(row.id).or_insert((0.0, 0.0, None, row));
            e.1 = score;
            e.2 = s.watched;
        }
        let scored = by_row
            .into_values()
            .map(|(tmdb, simkl, watched, row)| {
                let base = 0.5 * tmdb + 0.3 * simkl + 0.2 * self.seeders_score(row);
                let reason = watched
                    .filter(|w| *w > 0)
                    .map(|w| format!("{} watching today", compact(w)));
                (self.viewer.boost(row, base), row, reason)
            })
            .collect();
        self.shelf("trending", "Trending now", None, scored)
    }

    /// A single-list shelf ranked by list position, seeders and taste.
    fn list_shelf(
        &mut self,
        list: &str,
        key: &str,
        title: &str,
        kind: Option<&str>,
    ) -> Option<Shelf> {
        let scored = self
            .listed(list)
            .into_iter()
            .map(|(score, _, row)| {
                let base = 0.7 * score + 0.3 * self.seeders_score(row);
                (self.viewer.boost(row, base), row, None)
            })
            .collect();
        self.shelf(key, title, kind, scored)
    }

    /// "Hot on our trackers": this week's uploads, by swarm size.
    fn hot(&mut self) -> Option<Shelf> {
        let since = Utc::now() - chrono::Duration::days(HOT_WINDOW_DAYS);
        let scored = self
            .catalog
            .rows
            .iter()
            .filter(|r| !r.is_anime && r.released_at.is_some_and(|t| t >= since))
            .map(|r| {
                let reason = r
                    .seeders
                    .filter(|n| *n > 0)
                    .map(|n| format!("{} seeders", compact(n)));
                (self.viewer.boost(r, self.seeders_score(r)), r, reason)
            })
            .collect();
        self.shelf("hot", "Hot on our trackers", None, scored)
    }

    /// Freshest anime uploads (only reaches the account when opted in).
    fn new_anime(&mut self) -> Option<Shelf> {
        let since = Utc::now() - chrono::Duration::days(HOT_WINDOW_DAYS * 2);
        let scored = self
            .catalog
            .rows
            .iter()
            .filter(|r| r.is_anime && r.released_at.is_some_and(|t| t >= since))
            .map(|r| (self.seeders_score(r) + 0.5 * popularity(r), r, None))
            .collect();
        self.shelf("new_anime", "New anime", None, scored)
    }
}

async fn signals(pool: &SqlitePool) -> Result<Vec<Signal>, sqlx::Error> {
    iris_db::pulse::all_signals(pool).await
}

/// HOME — the pulse at a glance. Cached per user.
pub async fn for_you(state: &AppState, user_id: UserId) -> Result<ForYou, sqlx::Error> {
    build(state, user_id, Surface::Home).await
}

/// FOR-YOU PAGE — every discovery shelf.
pub async fn for_you_page(state: &AppState, user_id: UserId) -> Result<ForYou, sqlx::Error> {
    build(state, user_id, Surface::Page).await
}

async fn build(state: &AppState, user_id: UserId, surface: Surface) -> Result<ForYou, sqlx::Error> {
    let uuid: Uuid = user_id.into();
    if let Some(hit) = cache_get(uuid, surface) {
        return Ok(hit);
    }
    let viewer = Viewer::load(state, user_id).await?;
    let catalog = Catalog::load(state.db()).await?;
    let signals = signals(state.db()).await?;
    let mut a = Assembly::new(&viewer, &catalog, &signals);

    let mut shelves = vec![
        a.trending(),
        a.list_shelf("digital", "digital", "Just out on streaming", Some("movie")),
    ];
    if surface == Surface::Page {
        shelves.push(a.list_shelf("on_air", "on_air", "New episodes this week", Some("tv")));
    }
    shelves.push(a.hot());
    if surface == Surface::Page {
        shelves.push(a.new_anime());
    }
    let result = ForYou {
        shelves: shelves.into_iter().flatten().collect(),
    };
    cache_put(uuid, surface, result.clone());
    Ok(result)
}

/// A mood's grabbable titles for `kind`, best first: its TMDB discover list
/// joined with the catalogue, plus every catalogue title its genre rule
/// matches. Popularity, swarm and recency — the mood itself is the taste.
fn mood_ranking<'a>(
    viewer: &Viewer,
    catalog: &'a Catalog,
    signals: &[Signal],
    mood: &Mood,
    rule: GenreRule,
    kind: TmdbKind,
) -> Vec<&'a CatalogItem> {
    let kind = kind.as_wire();
    let list = mood.list();
    let listed: HashSet<Uuid> = signals
        .iter()
        .filter(|s| s.list == list && s.kind == kind)
        .filter_map(|s| catalog.get(s.tmdb_id, kind).map(|r| r.id))
        .collect();
    let max_seeders = catalog.max_seeders();
    let candidates: Vec<&CatalogItem> = catalog
        .rows
        .iter()
        .filter(|r| r.kind == kind && (listed.contains(&r.id) || rule.matches(&genre_ids(r))))
        .filter(|r| viewer.admits(r))
        .collect();
    let max_pop = candidates
        .iter()
        .filter_map(|r| r.popularity)
        .fold(0.0, f64::max);
    let year = Utc::now().year();
    let mut scored: Vec<(f64, &CatalogItem)> = candidates
        .into_iter()
        .map(|r| {
            let pop = if max_pop > 0.0 {
                r.popularity.unwrap_or(0.0) / max_pop
            } else {
                0.0
            };
            let listed_bonus = if listed.contains(&r.id) { 0.1 } else { 0.0 };
            let s = 0.45 * pop
                + 0.3 * seeders_score(r.seeders, max_seeders)
                + 0.15 * content_recency(item_year(r), year)
                + listed_bonus;
            (s, r)
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.into_iter().map(|(_, r)| r).collect()
}

/// The mood board for `kind`: every curated mood with enough to grab, the
/// account's genres first (curated order on ties), each tile showing the
/// mood's title of the moment.
pub async fn mood_board(
    state: &AppState,
    user_id: UserId,
    kind: TmdbKind,
) -> Result<MoodBoard, sqlx::Error> {
    let viewer = Viewer::load(state, user_id).await?;
    let catalog = Catalog::load(state.db()).await?;
    let signals = signals(state.db()).await?;

    let mut ranked: Vec<(f64, MoodTile)> = Vec::new();
    let mut used_backdrops: HashSet<String> = HashSet::new();
    for mood in MOODS {
        let Some(rule) = mood.rule(kind) else {
            continue;
        };
        let titles = mood_ranking(&viewer, &catalog, &signals, mood, rule, kind);
        if titles.len() < MOOD_MIN {
            continue;
        }
        // First backdrop not already on another tile: a horror sci-fi film
        // tops two moods, and two tiles sharing one image read as a bug.
        let featured = titles
            .iter()
            .find(|r| {
                image_url(r.backdrop_path.as_deref(), crate::tmdb::BACKDROP_SIZE)
                    .is_some_and(|u| !used_backdrops.contains(&u))
            })
            .or(titles.first());
        let backdrop = featured
            .and_then(|r| image_url(r.backdrop_path.as_deref(), crate::tmdb::BACKDROP_SIZE));
        if let Some(b) = &backdrop {
            used_backdrops.insert(b.clone());
        }
        let taste: f64 = rule.any.iter().filter_map(|g| viewer.affinity.get(g)).sum();
        ranked.push((
            taste,
            MoodTile {
                id: mood.id.to_string(),
                label: mood.label.to_string(),
                backdrop_url: backdrop,
                featured_title: featured.map(|r| r.title.clone()),
            },
        ));
    }
    // Stable: ties keep the curated order.
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
    Ok(MoodBoard {
        moods: ranked.into_iter().map(|(_, t)| t).collect(),
    })
}

/// A mood's grabbable titles for `kind`. Unknown mood → empty.
pub async fn mood_results(
    state: &AppState,
    user_id: UserId,
    mood_id: &str,
    kind: TmdbKind,
) -> Result<MoodResults, sqlx::Error> {
    let mut out = MoodResults {
        mood: mood_id.to_string(),
        kind: kind.as_wire().to_string(),
        items: Vec::new(),
    };
    let Some((mood, rule)) = MOODS
        .iter()
        .find(|m| m.id == mood_id)
        .and_then(|m| Some((m, m.rule(kind)?)))
    else {
        return Ok(out);
    };
    let viewer = Viewer::load(state, user_id).await?;
    let catalog = Catalog::load(state.db()).await?;
    let signals = signals(state.db()).await?;
    out.items = mood_ranking(&viewer, &catalog, &signals, mood, rule, kind)
        .into_iter()
        .take(MOOD_LIMIT)
        .map(|r| viewer.card(r, None))
        .collect();
    Ok(out)
}

/// 1.0 at the top of a list of `len`, falling linearly.
fn rank_score(rank: i64, len: i64) -> f64 {
    #[allow(clippy::cast_precision_loss)] // list positions are < 100
    let (rank, len) = (rank as f64, len as f64);
    (1.0 - rank / len).clamp(0.0, 1.0)
}

/// Log-scaled 0..1 swarm size; unknown (feeds without peers) sits mid-low.
fn seeders_score(seeders: Option<i64>, max: i64) -> f64 {
    let Some(n) = seeders else {
        return 0.3;
    };
    if max <= 0 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)] // seeder counts are small
    let (n, max) = (n.max(0) as f64, max as f64);
    (n.ln_1p() / max.ln_1p()).clamp(0.0, 1.0)
}

/// TMDB popularity squashed to 0..1 (it is unbounded; ~100 is very popular).
fn popularity(r: &CatalogItem) -> f64 {
    let p = r.popularity.unwrap_or(0.0).max(0.0);
    p / (p + 50.0)
}

/// 1.0 for the last ~2 years, down to 0.1 at 20 years old; unknown = 0.5.
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

fn item_year(item: &CatalogItem) -> Option<i32> {
    item.release_date
        .as_deref()
        .and_then(|d| d.split('-').next())
        .and_then(|y| y.parse().ok())
}

fn genre_ids(r: &CatalogItem) -> Vec<i64> {
    serde_json::from_str(&r.genres).unwrap_or_default()
}

/// `1792` → `1.8k`.
fn compact(n: i64) -> String {
    if n >= 1000 {
        #[allow(clippy::cast_precision_loss)]
        let k = n as f64 / 1000.0;
        format!("{k:.1}k").replace(".0k", "k")
    } else {
        n.to_string()
    }
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
        Some(crate::tmdb::image_url(p, size))
    }
}

/// Drop a user's cached shelves — called when prefs change or a card is
/// dismissed, so the next request rebuilds immediately.
pub(crate) fn invalidate(user_id: UserId) {
    let uuid: Uuid = user_id.into();
    if let Ok(mut guard) = CACHE.lock() {
        guard.retain(|(u, _), _| *u != uuid);
    }
}

fn card(row: &CatalogItem) -> CatalogCard {
    CatalogCard {
        catalog_id: row.id,
        tmdb_id: row.tmdb_id,
        // `catalog_items.kind` is CHECK-constrained to 'movie'/'tv'.
        kind: MediaKind::from_wire(&row.kind).unwrap_or(MediaKind::Tv),
        title: row.title.clone(),
        poster_url: image_url(row.poster_path.as_deref(), crate::tmdb::POSTER_SIZE),
        backdrop_url: image_url(row.backdrop_path.as_deref(), crate::tmdb::BACKDROP_SIZE),
        overview: row.overview.clone(),
        is_anime: row.is_anime,
        availability: row.availability.clone(),
        // Cards are title-references, never a specific recorded release. The
        // catalogue's "best release" comes from one feed slice or one search
        // and is frequently an aberrant 4K REMUX, so it is never offered
        // directly: `provider_id == null` routes web and TV to search, which
        // ranks a saner release first (`recommended_cmp`).
        seeders: None,
        provider_id: None,
        external_id: None,
        year: item_year(row),
        already_in_library: false,
        library_infohash: None,
        reason: None,
    }
}

fn cache_get(user: Uuid, surface: Surface) -> Option<ForYou> {
    let guard = CACHE.lock().ok()?;
    let (at, value) = guard.get(&(user, surface))?;
    (at.elapsed() < CACHE_TTL).then(|| value.clone())
}

fn cache_put(user: Uuid, surface: Surface, value: ForYou) {
    if let Ok(mut guard) = CACHE.lock() {
        guard.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
        guard.insert((user, surface), (Instant::now(), value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(tmdb_id: i64, language: &str, is_anime: bool) -> CatalogItem {
        CatalogItem {
            id: Uuid::new_v4(),
            tmdb_id: Some(tmdb_id),
            anilist_id: None,
            kind: "movie".to_string(),
            title: format!("t{tmdb_id}"),
            original_language: None,
            genres: "[27,18]".to_string(),
            is_anime,
            poster_path: None,
            backdrop_path: None,
            overview: None,
            popularity: None,
            vote_average: None,
            release_date: Some("2026-01-01".to_string()),
            availability: "available".to_string(),
            available_provider: None,
            seeders: Some(10),
            provider_id: Some("p".to_string()),
            external_id: Some("e".to_string()),
            download_url: None,
            infohash: None,
            language: Some(language.to_string()),
            released_at: None,
            size_bytes: None,
        }
    }

    fn viewer(languages: &[Language]) -> Viewer {
        Viewer {
            languages: languages.to_vec(),
            include_anime: false,
            watched: HashSet::from([7]),
            dismissed: HashSet::new(),
            library: HashSet::from([8]),
            affinity: HashMap::from([(27, 2.0)]),
            max_affinity: 2.0,
        }
    }

    #[test]
    fn viewer_filters_by_account() {
        let fr = viewer(&[Language::French]);
        assert!(fr.admits(&row(1, "french", false)));
        assert!(fr.admits(&row(1, "multi", false)));
        assert!(!fr.admits(&row(1, "english", false)));
        assert!(
            !fr.admits(&row(7, "french", false)),
            "watched by this account"
        );
        assert!(
            !fr.admits(&row(1, "french", true)),
            "anime needs the opt-in"
        );
        assert!(
            viewer(&[]).admits(&row(1, "english", false)),
            "no preference = any"
        );
    }

    #[test]
    fn library_titles_are_marked_not_hidden() {
        let v = viewer(&[]);
        let c = v.card(&row(8, "french", false), Some("x".to_string()));
        assert!(c.already_in_library);
        assert_eq!(c.reason.as_deref(), Some("In the library"));
        assert!(c.provider_id.is_none(), "cards route to search");
        assert!((v.boost(&row(1, "french", false), 1.0) - 1.5).abs() < 1e-9);
    }

    #[test]
    fn scores_are_bounded() {
        assert!((rank_score(0, 40) - 1.0).abs() < 1e-9);
        assert!(rank_score(39, 40) > 0.0);
        assert!((seeders_score(Some(100), 100) - 1.0).abs() < 1e-9);
        assert!(seeders_score(Some(1), 100) < 0.2);
        assert_eq!(compact(1792), "1.8k");
        assert_eq!(compact(2000), "2k");
        assert_eq!(compact(950), "950");
    }
}
