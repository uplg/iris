// Season / episode / seeders / size casts move between i64 and
// u32/u64 — domain values are positive and bounded, so pedantic
// cast warnings are noise.
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
)]

//! Notify-only scheduler for TV `collections`.
//!
//! Wakes every 4 hours and walks every TV collection that has at
//! least one parsed title (i.e. the household actually ingested an
//! episode of it). For each collection, asks the indexer for
//! everything matching the SCENE name and stashes new (S, E) hits
//! into `available_episodes` so the eventual user click on
//! "Prepare" / "Play next" doesn't gate on a fresh indexer query.
//!
//! Replaces the previous `series_follows`-driven scheduler. The
//! "follow" concept was retired: any TV collection now auto-arms
//! the indexer watch, so the user discovers new episodes without
//! ever having to know what a follow is. The `series_follows`
//! table sticks around through the 0.4 cycle to keep APK 0.3.1
//! happy (see `routes/follows.rs` façade), then gets dropped.
//!
//! Pure SCENE pipeline — no TMDB call, no expected-episode grid.
//! "What episodes exist" is whatever the indexer returns when
//! searching the show's name. Episodes the indexer doesn't know
//! about don't appear; that's correct because Iris can't grab
//! something the indexer doesn't list anyway.
//!
//! Crucially this scheduler **never** triggers an ingest. Auto-grab
//! was explicitly rejected at planning time — the user wants to be
//! notified and stay in control of disk usage. The on-demand grab
//! endpoint (`/api/library/collections/:id/grab/:s/:e`) is what
//! actually pulls a torrent.

use std::sync::Arc;
use std::time::Duration;

use iris_core::search::{SearchQuery, SearchResult, SortField, SortOrder};
use iris_db::SqlitePool;
use iris_db::collections::CollectionRow;
use iris_media::filename;
use iris_media::filename::Language;
use iris_providers::ProviderRegistry;
use uuid::Uuid;

/// View a search result through the shared "recommended" ordering lens
/// (seeders + size + `MULTi`). `is_multi` is passed in because the caller
/// already resolved the language for the `(S, E, language)` bucket.
fn candidate_of(r: &SearchResult, is_multi: bool) -> iris_core::ranking::Candidate {
    iris_core::ranking::Candidate {
        seeders: r.seeders.map(i64::from),
        size_bytes: r.size_bytes.map(|b| i64::try_from(b).unwrap_or(i64::MAX)),
        is_multi,
    }
}

/// How often to scan all TV collections. Most series ship a new
/// episode once a week, so 4 h means we surface a release within
/// hours of it landing on a tracker.
const TICK_INTERVAL: Duration = Duration::from_hours(4);

/// Skip per-collection work if `last_indexer_scan_at` is younger
/// than this. Two hours strikes the same balance as the retired
/// follows cooldown: re-scan often enough that a manual page
/// refresh tends to hit fresh data, but not so often that an idle
/// browser tab burns indexer credits.
const PER_COLLECTION_COOLDOWN_SECS: i64 = 2 * 60 * 60;

pub fn spawn(pool: SqlitePool, providers: ProviderRegistry) {
    let providers = Arc::new(providers);
    tokio::spawn(async move {
        // Initial pass after a short warm-up so the library shelves
        // get their `available_episodes` populated within seconds of
        // boot, not at the 4 h mark.
        tokio::time::sleep(Duration::from_secs(10)).await;
        run_pass(&pool, providers.clone()).await;

        let mut ticker = tokio::time::interval(TICK_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        ticker.tick().await; // skip the immediate firing
        loop {
            ticker.tick().await;
            run_pass(&pool, providers.clone()).await;
        }
    });
    tracing::info!(
        interval_secs = TICK_INTERVAL.as_secs(),
        "collections notify scheduler started"
    );
}

async fn run_pass(pool: &SqlitePool, providers: Arc<ProviderRegistry>) {
    let collections =
        match iris_db::collections::list_due_for_scan(pool, PER_COLLECTION_COOLDOWN_SECS).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "scheduler: list due collections failed");
                return;
            }
        };
    if collections.is_empty() {
        return;
    }
    tracing::info!(
        count = collections.len(),
        "scheduler: scanning TV collections"
    );
    for collection in collections {
        if let Err(e) = check_one(pool, &providers, &collection).await {
            tracing::warn!(
                collection_id = %collection.id,
                display = %collection.display_title,
                error = %e,
                "scheduler: collection scan failed",
            );
        }
        // Stamp regardless of success: a permanently-broken indexer
        // mustn't make the scheduler hammer the same collection on
        // every tick.
        let _ = iris_db::collections::touch_scanned(pool, collection.id).await;
    }
}

/// Public entry-point — scan one collection on demand. Used by
/// ingest-time hooks so a newly-grouped TV collection shows its
/// available episodes on first visit instead of waiting on the 4h
/// periodic tick.
pub async fn scan_collection(
    pool: &SqlitePool,
    providers: &ProviderRegistry,
    collection_id: Uuid,
) -> anyhow::Result<()> {
    let Some(collection) = iris_db::collections::get(pool, collection_id).await? else {
        return Ok(());
    };
    if collection.kind != "tv" {
        return Ok(());
    }
    check_one(pool, providers, &collection).await?;
    let _ = iris_db::collections::touch_scanned(pool, collection.id).await;
    Ok(())
}

async fn check_one(
    pool: &SqlitePool,
    providers: &ProviderRegistry,
    collection: &CollectionRow,
) -> anyhow::Result<()> {
    let Some(normalized) = collection.parsed_title_normalized.as_deref() else {
        // No SCENE identity → can't query the indexer with confidence.
        // These collections exist for the long-tail "standalone"
        // ingest path and aren't candidates for episode watching.
        return Ok(());
    };
    // The collections-level language preference used to filter
    // results out here. We no longer do that — the household runs
    // 2 anglophone + 8 francophone users on a shared library, so a
    // hard FR filter would silently strip Seedpool's English
    // releases (the only EN source) before they ever reached the
    // anglophone users' Watchlist. The new shape stores best per
    // (S, E, language) and lets the UI render badges so users pick
    // explicitly.
    // Single broad search per collection — `<display title>`. The
    // indexer returns whatever episodes / packs / specials it has;
    // we SCENE-parse each title to extract (S, E) and dedup.
    let q = SearchQuery {
        q: collection.display_title.clone(),
        page: Some(1),
        limit: Some(100),
        sort_by: Some(SortField::Seeders),
        order: Some(SortOrder::Desc),
        kind: None,
        // Scheduler intentionally queries by show name only — providers
        // shouldn't narrow to a specific S/E here.
        parsed_title: None,
        season: None,
        episode: None,
        year: None,
    };
    let agg = providers.search_all(&q).await;
    if agg.results.is_empty() {
        return Ok(());
    }

    // Group results by (S, E, language); within each group pick
    // the highest-seeded entry. Normalised title match keeps
    // unrelated indexer noise (e.g., "Squid Game Documentary")
    // out. No language filter — Seedpool's EN releases coexist
    // with c411's FR releases, and the UI shows badges so the
    // user picks consciously. Untagged releases fall through to
    // the provider's configured default language (`seedpool` ships
    // English implicitly).
    let mut best: std::collections::HashMap<(i64, i64, Language), SearchResult> =
        std::collections::HashMap::new();
    for r in agg.results {
        let Some(parsed) = filename::parse(&r.title) else {
            continue;
        };
        // Use the TV-side anime-aware `collection_key_kind` so a SCENE
        // name like `Lucky.Luke.1991.S01E01...` (which normalises to
        // `"lucky luke 1991"`) still matches a collection whose
        // `parsed_title_normalized` was derived from the original
        // ingest filename (= `"lucky luke"`), AND so an anime
        // collection only ingests anime offers: a live-action
        // `One Piece S01E01` keys to `"one piece"` while the anime
        // collection is `"anime:one piece"` (and vice-versa). This
        // per-result identity check is what stops the cross-entity
        // 23-"season" dump.
        let result_is_anime =
            filename::looks_like_anime_release(&r.title, parsed.season, parsed.episode);
        if parsed.collection_key_kind(true, result_is_anime) != normalized {
            continue;
        }
        let Some(s) = parsed.season else { continue };
        let Some(e) = parsed.episode else { continue };
        // `episode == 0` is the SCENE parser's season-pack sentinel.
        // Stored alongside individual episodes so the grab path can
        // fall back to "ingest the pack, find the requested episode
        // inside" when no singleton offer exists. The API splits
        // packs into a separate `season_packs` field — the UI never
        // shows them as episode rows.
        let detected = filename::detect_language(&r.title);
        let lang = if detected == Language::Unknown {
            providers
                .default_language(&r.provider_id)
                .map_or(detected, Language::parse_tag)
        } else {
            detected
        };
        let key = (i64::from(s), i64::from(e), lang);
        // Keep the recommended-best per (S, E, language): smallest sane
        // size first, seeders only as a garde-fou. Without this a 51 GB
        // 4K pack with the most seeders would displace a healthy 8 GB
        // 1080p one *before* it ever reached `available_episodes`.
        let is_multi = lang == Language::Multi;
        let replace = best.get(&key).is_none_or(|existing| {
            iris_core::ranking::recommended_cmp(
                &candidate_of(&r, is_multi),
                &candidate_of(existing, is_multi),
            ) == std::cmp::Ordering::Less
        });
        if replace {
            best.insert(key, r);
        }
    }

    for ((season, episode, lang), result) in best {
        // Always upsert — re-recording refreshes the cached seeders/size on
        // an offer we've seen before. Without this, seeders are frozen at
        // first-seen: a pack that since died to 0 would keep being offered,
        // and the 0-seeder filter in `list_season_packs_for_series` would
        // never catch it. `upsert` preserves the original `found_at`, so
        // refreshing an existing offer doesn't re-flag it "new".
        record_availability(pool, providers, normalized, season, episode, lang, &result).await;
    }
    Ok(())
}

async fn record_availability(
    pool: &SqlitePool,
    providers: &ProviderRegistry,
    normalized_name: &str,
    season: i64,
    episode: i64,
    language: Language,
    best: &SearchResult,
) {
    if !providers.ids().iter().any(|id| id == &best.provider_id) {
        return;
    }
    // Absolute number for fleuve anime offers (threshold-gated in the
    // helper); seasonal anime + ordinary TV stay `None`. Powers the
    // flat "Episode N" list on the collection page.
    let absolute_episode = filename::parse(&best.title)
        .as_ref()
        .and_then(filename::absolute_from_parsed)
        .map(i64::from);
    let upsert = iris_db::available_episodes::UpsertAvailableEpisode {
        normalized_name: normalized_name.to_string(),
        season,
        episode,
        indexer_provider: best.provider_id.clone(),
        indexer_torrent_id: best.external_id.clone(),
        magnet: String::new(),
        quality: best
            .tags
            .iter()
            .find(|t| t.contains("1080p") || t.contains("720p") || t.contains("2160p"))
            .cloned()
            .or_else(|| extract_quality_from_title(&best.title)),
        seeders: best.seeders.map(i64::from),
        size_bytes: best.size_bytes.map(|s| s as i64),
        language: Some(language.as_str().to_string()),
        // Persist the Torznab / UNIT3D `.torrent` URL so the grab
        // path survives the next restart even if the provider's
        // in-memory link cache evaporated. torr9 will be None
        // here — its resolve() fetches per-id anyway.
        download_url: best.download_url.clone(),
        absolute_episode,
    };
    if let Err(e) = iris_db::available_episodes::upsert(pool, upsert).await {
        tracing::warn!(error = %e, "scheduler: upsert availability failed");
    } else {
        // Debug, not info: this now fires for every (S, E, language) on every
        // scan (we re-record to refresh seeders), so info would spam the log
        // every 4 h with the whole library.
        tracing::debug!(
            normalized_name, season, episode,
            provider = %best.provider_id,
            seeders = best.seeders.unwrap_or(0),
            language = language.as_str(),
            "scheduler: recorded/refreshed episode availability",
        );
    }
}

fn extract_quality_from_title(title: &str) -> Option<String> {
    for q in ["2160p", "1080p", "720p", "480p"] {
        if title.contains(q) {
            return Some(q.to_string());
        }
    }
    None
}
