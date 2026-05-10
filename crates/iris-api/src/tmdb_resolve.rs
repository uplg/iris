//! SCENE-name → TMDB resolution with persistent cache.
//!
//! The indexer-supplied `tmdb_id`s on torr9 results are unreliable —
//! Silicon Valley releases come back tagged with The Burning Bed's id,
//! etc. This module derives the canonical TMDB id from the *release
//! name itself*, which is the authoritative identifier on every
//! tracker.
//!
//! Flow:
//!   1. Parse the SCENE-style name with `iris_media::filename::parse`
//!      to extract `(title, year, season?)`. The presence of an SxxExx
//!      marker doubles as the kind hint (TV vs movie) when callers
//!      don't supply one.
//!   2. Look up the `(cleaned_name, kind_hint)` pair in the persistent
//!      `tmdb_resolve_cache` table. Hits within `MAX_AGE` short-
//!      circuit; negative entries (resolved-as-not-found) prevent
//!      retries on every render.
//!   3. On miss, call `tmdb.multi_search`, score the candidates
//!      against the parsed kind / year, persist the top match (or a
//!      negative-cache row), and return it.

use chrono::{Duration, Utc};
use iris_db::SqlitePool;
use iris_db::tmdb_cache::{self, ResolveEntry};

use crate::tmdb::{TmdbClient, TmdbKind, TmdbSuggestion};

/// 30 days. TMDB poster paths and base metadata are essentially static
/// for a given title, but we re-resolve occasionally to pick up newly-
/// added entries (a fresh release whose TMDB page didn't exist when we
/// first cached the negative).
const MAX_AGE_DAYS: i64 = 30;

/// Resolve `release_name` to a TMDB suggestion. `kind_hint` overrides
/// the parsed-from-name kind (set it when the caller has stronger
/// evidence — e.g. the search-result `kind` filter, or an existing
/// follow's classification). Returns `None` only when neither the
/// cache nor TMDB had anything for this title.
pub async fn resolve_release_name(
    pool: &SqlitePool,
    tmdb: &TmdbClient,
    release_name: &str,
    kind_hint: Option<TmdbKind>,
) -> Option<ResolvedTitle> {
    let parsed = iris_media::filename::parse(release_name)?;
    let cleaned = iris_media::filename::series_key(&parsed.title);
    if cleaned.len() < 2 {
        return None;
    }
    let resolved_kind = kind_hint.or_else(|| {
        if parsed.is_tv() { Some(TmdbKind::Tv) } else { Some(TmdbKind::Movie) }
    });
    resolve_cleaned(pool, tmdb, &cleaned, resolved_kind, parsed.year.map(u32::from)).await
}

/// Lower-level entry point: resolve directly from a pre-cleaned title
/// (already passed through `iris_media::normalize_title` /
/// `series_key`). Used by the search-page client path where the cards
/// hand us a SCENE-extracted name.
pub async fn resolve_cleaned(
    pool: &SqlitePool,
    tmdb: &TmdbClient,
    cleaned: &str,
    kind_hint: Option<TmdbKind>,
    year_hint: Option<u32>,
) -> Option<ResolvedTitle> {
    let kind_str = kind_hint.map(kind_to_str);
    let max_age = Duration::days(MAX_AGE_DAYS);

    if let Ok(Some(hit)) = tmdb_cache::get(pool, cleaned, kind_str, max_age).await {
        return ResolvedTitle::from_entry(&hit, kind_hint);
    }

    let suggestions = tmdb.multi_search(cleaned).await;
    let top = pick_best(&suggestions, kind_hint, year_hint);
    let entry = match top.as_ref() {
        Some(t) => ResolveEntry {
            tmdb_id: i64::try_from(t.tmdb_id).ok(),
            title: Some(t.title.clone()),
            year: t.year.and_then(|y| i64::try_from(y).ok()),
            poster_path: t.poster_path.clone(),
            backdrop_path: None, // multi_search doesn't return backdrops; ok to leave
            overview: t.overview.clone(),
            fetched_at: Utc::now(),
        },
        None => ResolveEntry::not_found_at(Utc::now()),
    };
    if let Err(e) = tmdb_cache::put(pool, cleaned, kind_str, &entry).await {
        tracing::warn!(error = %e, cleaned, "tmdb_resolve_cache put failed");
    }
    top.map(|t| ResolvedTitle {
        tmdb_id: t.tmdb_id,
        kind: t.kind,
        title: t.title,
        year: t.year,
        poster_path: t.poster_path,
        overview: t.overview,
    })
}

#[derive(Debug, Clone)]
pub struct ResolvedTitle {
    pub tmdb_id: u64,
    pub kind: TmdbKind,
    pub title: String,
    pub year: Option<u32>,
    pub poster_path: Option<String>,
    pub overview: Option<String>,
}

impl ResolvedTitle {
    fn from_entry(entry: &ResolveEntry, kind_hint: Option<TmdbKind>) -> Option<Self> {
        let id_i64 = entry.tmdb_id?;
        let tmdb_id = u64::try_from(id_i64).ok()?;
        Some(Self {
            tmdb_id,
            kind: kind_hint.unwrap_or(TmdbKind::Movie),
            title: entry.title.clone().unwrap_or_default(),
            year: entry.year.and_then(|y| u32::try_from(y).ok()),
            poster_path: entry.poster_path.clone(),
            overview: entry.overview.clone(),
        })
    }
}

fn pick_best(
    suggestions: &[TmdbSuggestion],
    kind_hint: Option<TmdbKind>,
    year_hint: Option<u32>,
) -> Option<TmdbSuggestion> {
    if suggestions.is_empty() {
        return None;
    }
    // 1) kind + year exact match
    // 2) kind match, closest year
    // 3) any kind, year match
    // 4) first
    if let Some(kh) = kind_hint {
        let kind_match: Vec<&TmdbSuggestion> = suggestions
            .iter()
            .filter(|s| same_kind(s.kind, kh))
            .collect();
        if !kind_match.is_empty() {
            if let Some(yh) = year_hint {
                if let Some(exact) = kind_match.iter().find(|s| s.year == Some(yh)) {
                    return Some((*exact).clone());
                }
                // Closest year (prefers ±1 over ±5, etc.)
                let closest = kind_match
                    .iter()
                    .min_by_key(|s| s.year.map_or(u32::MAX, |y| y.abs_diff(yh)));
                if let Some(c) = closest {
                    return Some((*c).clone());
                }
            }
            return Some(kind_match[0].clone());
        }
    }
    if let Some(yh) = year_hint {
        if let Some(s) = suggestions.iter().find(|s| s.year == Some(yh)) {
            return Some(s.clone());
        }
    }
    suggestions.first().cloned()
}

fn same_kind(a: TmdbKind, b: TmdbKind) -> bool {
    matches!(
        (a, b),
        (TmdbKind::Movie, TmdbKind::Movie) | (TmdbKind::Tv, TmdbKind::Tv)
    )
}

fn kind_to_str(k: TmdbKind) -> &'static str {
    match k {
        TmdbKind::Movie => "movie",
        TmdbKind::Tv => "tv",
    }
}
