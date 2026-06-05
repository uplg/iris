//! Discovery freshness scheduler — the tracker RSS "rolling window".
//!
//! This is the tracker-first inversion of the old metadata-only seeding: the
//! candidate universe is **what the trackers actually have right now**, not
//! TMDB's trending list. Every tick polls one provider's latest-releases feed
//! for one kind (`provider.latest()` — torr9 RSS, UNIT3D `created_at`, Torznab
//! query-less). One (provider × kind) slice per tick keeps the load spread out
//! ("au fil de l'eau"), never bursting all trackers at once.
//!
//! For each fresh release in the window we:
//!   1. drop non-video + dead (0-seeder) releases,
//!   2. reconcile the SCENE name to TMDB (persistent-cached `resolve_release_name`),
//!   3. keep the best release per title (shared `recommended_cmp`),
//!   4. enrich with full TMDB metadata and upsert into `catalog_items` with
//!      `availability='available'` + the grab facts.
//!
//! So the discovery shelves only ever surface titles a tracker can serve —
//! disponibilité garantie. A full cycle ends with a sliding-window GC. TMDB is
//! correlation only (poster / genres / popularity); it is never the source.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Datelike, Utc};
use iris_config::DiscoveryConfig;
use iris_core::search::{MediaKind, SearchResult};
use iris_db::SqlitePool;
use iris_media::filename::{self, Language};
use iris_providers::ProviderRegistry;

use crate::anilist::{AniListClient, AniListMedia};
use crate::tmdb::{MediaMetadata, TmdbClient, TmdbKind};
use crate::tmdb_resolve;

/// Each full cycle polls every provider for movies then TV.
const KINDS: [MediaKind; 2] = [MediaKind::Movie, MediaKind::Tv];

/// Lazy reco candidates (`availability='unknown'`, no `released_at`) are GC'd
/// after this long without a refresh. They're re-derived on request, so a
/// short window keeps the table lean.
const LAZY_TTL_DAYS: i64 = 14;

/// View a search result through the shared "recommended" ordering lens
/// (smallest sane size first, seeders only as a garde-fou, `MULTi` discounted).
fn candidate_of(r: &SearchResult, is_multi: bool) -> iris_core::ranking::Candidate {
    iris_core::ranking::Candidate {
        seeders: r.seeders.map(i64::from),
        size_bytes: r.size_bytes.and_then(|b| i64::try_from(b).ok()),
        is_multi,
    }
}

const fn tmdb_kind(kind: MediaKind) -> TmdbKind {
    match kind {
        MediaKind::Movie => TmdbKind::Movie,
        MediaKind::Tv => TmdbKind::Tv,
    }
}

const fn kind_str(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Movie => "movie",
        MediaKind::Tv => "tv",
    }
}

pub fn spawn(pool: SqlitePool, tmdb: TmdbClient, providers: ProviderRegistry, cfg: DiscoveryConfig) {
    let providers = Arc::new(providers);
    // Keyless AniList client for anime correlation (precise poster + the
    // anime dedup identity). Absence just disables the anime category.
    let anilist = match AniListClient::new() {
        Ok(c) => Some(c),
        Err(e) => {
            tracing::warn!(error = %e, "anilist init failed; anime correlation disabled");
            None
        }
    };
    tokio::spawn(async move {
        // One slice per (provider, kind); a full pass over `slices` is a cycle.
        let slices: Vec<(String, MediaKind)> = providers
            .ids()
            .into_iter()
            .flat_map(|id| KINDS.iter().map(move |k| (id.clone(), *k)))
            .collect();
        if slices.is_empty() {
            tracing::info!("freshness scheduler: no providers; not starting");
            return;
        }

        tokio::time::sleep(Duration::from_secs(15)).await; // boot warm-up

        let interval = Duration::from_secs(cfg.slice_interval_minutes.max(1) * 60);
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut idx = 0usize;
        loop {
            ticker.tick().await; // first tick fires immediately (after warm-up)
            let (provider_id, kind) = &slices[idx % slices.len()];
            run_slice(&pool, &tmdb, anilist.as_ref(), &providers, provider_id, *kind, &cfg).await;
            idx += 1;
            if idx % slices.len() == 0 {
                run_gc(&pool, &cfg).await; // end of a full cycle
            }
        }
    });
    tracing::info!("freshness (rolling-window) scheduler started");
}

async fn run_slice(
    pool: &SqlitePool,
    tmdb: &TmdbClient,
    anilist: Option<&AniListClient>,
    providers: &ProviderRegistry,
    provider_id: &str,
    kind: MediaKind,
    cfg: &DiscoveryConfig,
) {
    let Some(provider) = providers.get(provider_id) else {
        return;
    };
    let page = match provider.latest(Some(kind), 1).await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(provider = provider_id, ?kind, error = %e, "freshness: latest() failed");
            return;
        }
    };
    if page.results.is_empty() {
        return;
    }

    let window_start = Utc::now() - chrono::Duration::weeks(cfg.poll_window_weeks.max(1));
    let best = collect_best(pool, tmdb, providers, provider_id, kind, page.results, window_start).await;
    let upserted =
        upsert_window_rows(pool, tmdb, anilist, provider_id, kind, best, cfg.max_content_age_years).await;

    tracing::info!(
        provider = provider_id,
        ?kind,
        upserted,
        "freshness: slice complete"
    );
}

/// Dedup a provider's latest feed to the best release per resolved TMDB id
/// (within the window, video-only, identifiable on TMDB).
async fn collect_best(
    pool: &SqlitePool,
    tmdb: &TmdbClient,
    providers: &ProviderRegistry,
    provider_id: &str,
    kind: MediaKind,
    results: Vec<SearchResult>,
    window_start: DateTime<Utc>,
) -> HashMap<i64, (SearchResult, Language)> {
    let tk = tmdb_kind(kind);
    let mut best: HashMap<i64, (SearchResult, Language)> = HashMap::new();
    for r in results {
        // Rolling window: skip anything older than the poll horizon. Releases
        // with no upload date (rare) are kept — they came off a "latest" feed.
        if r.uploaded_at.is_some_and(|up| up < window_start) {
            continue;
        }
        if !r.is_probably_video() {
            continue;
        }
        // Reconcile to TMDB (persistent-cached). Releases TMDB can't identify
        // are skipped — a poster-driven shelf has nothing to show for them.
        let Some(resolved) =
            tmdb_resolve::resolve_release_name(pool, tmdb, &r.title, Some(tk)).await
        else {
            continue;
        };
        let Ok(tmdb_id) = i64::try_from(resolved.tmdb_id) else {
            continue;
        };

        // Language: detected from the SCENE name, falling back to the
        // provider's configured default for untagged releases.
        let detected = filename::detect_language(&r.title);
        let lang = if detected == Language::Unknown {
            providers
                .default_language(provider_id)
                .map_or(detected, Language::parse_tag)
        } else {
            detected
        };
        let is_multi = lang == Language::Multi;
        let replace = best.get(&tmdb_id).is_none_or(|(existing, ex_lang)| {
            iris_core::ranking::recommended_cmp(
                &candidate_of(&r, is_multi),
                &candidate_of(existing, *ex_lang == Language::Multi),
            ) == std::cmp::Ordering::Less
        });
        if replace {
            best.insert(tmdb_id, (r, lang));
        }
    }
    best
}

/// Enrich each best release with full TMDB metadata (+ AniList for anime) and
/// upsert it into `catalog_items` with `availability='available'`. Returns the
/// number of rows written.
async fn upsert_window_rows(
    pool: &SqlitePool,
    tmdb: &TmdbClient,
    anilist: Option<&AniListClient>,
    provider_id: &str,
    kind: MediaKind,
    best: HashMap<i64, (SearchResult, Language)>,
    max_content_age_years: i64,
) -> usize {
    let tk = tmdb_kind(kind);
    // Movies older than this content-year are kept out of the discovery window
    // (the window is about recent *releases*, not old re-uploads). TV is exempt
    // — a long-running series airing a new episode is legitimately fresh.
    let movie_cutoff_year =
        Utc::now().year() - i32::try_from(max_content_age_years.max(0)).unwrap_or(0);
    let mut upserted = 0usize;
    for (tmdb_id, (release, lang)) in best {
        // Never store a known-dead release (0 seeders). torr9 RSS carries no
        // seeders (None) → stored as unknown; the grab-time re-check is the
        // authoritative gate.
        if release.seeders == Some(0) {
            continue;
        }
        let Ok(id_u64) = u64::try_from(tmdb_id) else {
            continue;
        };
        // Full metadata (genres/popularity/backdrop/original_language) — the
        // persistent resolve cache only carries poster/overview. Cached forever.
        let Some(meta) = tmdb.lookup_with_kind(id_u64, Some(tk)).await else {
            continue;
        };

        // Content-age floor (movies only): keep very old films out of
        // discovery even when freshly re-uploaded.
        if kind == MediaKind::Movie
            && meta
                .year
                .and_then(|y| i32::try_from(y).ok())
                .is_some_and(|y| y < movie_cutoff_year)
        {
            continue;
        }

        // Anime correlation: a TMDB-anime release (genre Animation + JA) is
        // reconciled to AniList for a precise poster + the anime dedup
        // identity. No AniList match → it stays a plain (non-anime) row so we
        // never create duplicate anime rows keyed on a NULL anilist_id.
        let anime = if is_anime_meta(&meta) {
            match anilist {
                Some(a) => pick_anime(&a.search(&meta.title).await, meta.year),
                None => None,
            }
        } else {
            None
        };

        let mut item = iris_db::catalog::NewCatalogItem {
            tmdb_id: Some(tmdb_id),
            anilist_id: None,
            kind: kind_str(kind).to_string(),
            title: meta.title,
            original_language: meta.original_language,
            genres: meta.genre_ids.iter().map(|&g| i64::from(g)).collect(),
            is_anime: false,
            poster_path: meta.poster_path,
            backdrop_path: meta.backdrop_path,
            overview: meta.overview,
            popularity: meta.popularity,
            vote_average: meta.vote_score,
            release_date: meta.release_date,
            source: Some(format!("freshness:{provider_id}:{}", kind_str(kind))),
            availability: "available".to_string(),
            seeders: release.seeders.map(i64::from),
            provider_id: Some(release.provider_id),
            external_id: Some(release.external_id),
            download_url: release.download_url,
            infohash: release.infohash,
            language: Some(lang.as_str().to_string()),
            released_at: release.uploaded_at,
        };

        let res = if let Some(am) = anime {
            // Route through the anime dedup space (keyed on anilist_id) and
            // prefer the AniList artwork.
            item.is_anime = true;
            item.anilist_id = Some(am.anilist_id);
            item.poster_path = am.cover_image.or_else(|| item.poster_path.take());
            item.backdrop_path = am.banner_image.or_else(|| item.backdrop_path.take());
            iris_db::catalog::upsert_anime(pool, &item).await
        } else {
            iris_db::catalog::upsert_item(pool, &item).await
        };
        match res {
            Ok(()) => upserted += 1,
            Err(e) => tracing::warn!(error = %e, "freshness: upsert failed"),
        }
    }
    upserted
}

/// Heuristic: does this TMDB title look like anime? Animation genre (16) in
/// Japanese. Good enough to gate the (cached) AniList reconciliation.
fn is_anime_meta(meta: &MediaMetadata) -> bool {
    meta.genre_ids.contains(&16) && meta.original_language.as_deref() == Some("ja")
}

/// Pick the AniList match for a title, preferring an exact release-year match.
fn pick_anime(results: &[AniListMedia], year: Option<u32>) -> Option<AniListMedia> {
    if let Some(y) = year.and_then(|y| u16::try_from(y).ok()) {
        if let Some(m) = results.iter().find(|m| m.year == Some(y)) {
            return Some(m.clone());
        }
    }
    results.first().cloned()
}

async fn run_gc(pool: &SqlitePool, cfg: &DiscoveryConfig) {
    let released_before = Utc::now() - chrono::Duration::weeks(cfg.retain_weeks.max(1));
    let lazy_before = Utc::now() - chrono::Duration::days(LAZY_TTL_DAYS);
    match iris_db::catalog::prune_window(pool, released_before, lazy_before).await {
        Ok(n) if n > 0 => tracing::info!(pruned = n, "freshness: window GC slid"),
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "freshness: window GC failed"),
    }
}
