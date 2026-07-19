//! Indexer pre-cache for episodes that aren't in the library yet.
//! Populated by the notify scheduler so a user's "Préparer E06"
//! click doesn't have to wait on a fresh indexer round-trip.
//!
//! Keyed on `normalized_name` (the SCENE-style normalised title)
//! to match the way `series_follows` is identified — TMDB id is
//! never used as a join key here.
//!
//! NOT per-user: if iris knows a SCENE-named series has S03E12
//! grabbable, every authorised user sees the same offer.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct AvailableEpisodeRow {
    pub id: Uuid,
    pub normalized_name: String,
    pub season: i64,
    pub episode: i64,
    pub indexer_provider: String,
    pub indexer_torrent_id: String,
    pub magnet: String,
    pub quality: Option<String>,
    pub seeders: Option<i64>,
    pub size_bytes: Option<i64>,
    pub found_at: DateTime<Utc>,
    /// Coarse language tag derived from the release title at scan
    /// time: `"french"` / `"english"` / `"multi"` / `"unknown"`.
    /// `None` only on legacy rows pre-migration 0017 — those read
    /// as Unknown downstream.
    #[serde(default)]
    pub language: Option<String>,
    /// Pre-signed `.torrent` download URL captured at scan time.
    /// Lets the grab path bypass the provider's in-memory link
    /// cache — that cache evaporates on restart and pre-0.4.0
    /// installs would 500 on the first grab attempt after each
    /// reboot. `None` for providers that don't surface a URL
    /// (torr9's JSON API), or on legacy rows pre-migration 0018.
    #[serde(default)]
    pub download_url: Option<String>,
    /// Absolute episode number for fleuve anime offers
    /// (`One Piece S01E1156` → 1156). `NULL` for ordinary seasonal
    /// releases and for packs. Powers the flat anime list.
    #[serde(default)]
    pub absolute_episode: Option<i64>,
    /// Coarse video codec parsed from the release title at scan time
    /// (`"h264"` / `"hevc"` / `"av1"` / `"vp9"` / `"unknown"`). `None`
    /// on legacy rows pre-migration 0035 — treated as unknown by the
    /// grab path's codec preference.
    #[serde(default)]
    pub codec: Option<String>,
}

/// SQL `ORDER BY` fragment mirroring `iris_core::ranking::recommended_cmp`,
/// so the DB grab path ranks releases exactly like the in-memory search
/// path. Both read the thresholds from `iris_core::ranking`, so the SQL
/// and the Rust comparator can't drift:
///   1. sane (alive + not junk-sized) before anything dodgy,
///   2. smallest effective size first (unknown size last; `MULTi`
///      discounted so it edges out a same-size single-language release),
///   3. more seeders as the tie-break.
fn recommended_order_sql() -> String {
    use iris_core::ranking::{MIN_SANE_BYTES, MULTI_SIZE_DISCOUNT, SEED_FLOOR};
    format!(
        "CASE WHEN (seeders IS NULL OR seeders >= {SEED_FLOOR}) \
              AND (size_bytes IS NULL OR size_bytes >= {MIN_SANE_BYTES}) \
             THEN 0 ELSE 1 END ASC, \
         (size_bytes IS NULL) ASC, \
         CASE WHEN language = 'multi' \
              THEN CAST(size_bytes AS REAL) / {MULTI_SIZE_DISCOUNT} \
              ELSE size_bytes END ASC, \
         COALESCE(seeders, 0) DESC"
    )
}

/// Best offer per `(normalized_name, season, episode, language)` — one
/// row per (episode × language) so the UI can render FR + EN side-by-side
/// and the user picks. The per-group winner is the **recommended-best**
/// (smallest sane size first, seeders as garde-fou, `MULTi` discounted),
/// picked in a single SQL pass via a `ROW_NUMBER` window — see
/// [`recommended_order_sql`]. This stops a 51 GB 4K release out-ranking a
/// healthy 8 GB 1080p one just because it has more seeders.
///
/// Legacy rows without a language tag fall into the `Unknown`
/// bucket (the column reads as NULL → grouped together → no
/// crash). They were almost certainly French-or-multi releases
/// captured before migration 0017; the UI badge will show
/// "unknown" until they age out of the scheduler's cache and get
/// re-stored with a real language.
///
/// Confirmed-dead offers (**exactly 0 seeders**) are excluded, same
/// contract as the pack listing: they're un-grabbable, so surfacing
/// them as "available" badges would only produce grab failures.
/// Unknown (NULL) seeder counts are kept.
pub async fn list_best_for_series(
    pool: &SqlitePool,
    normalized_name: &str,
) -> Result<Vec<AvailableEpisodeRow>, sqlx::Error> {
    // QueryBuilder rather than a `format!`-built string: the ranking
    // `ORDER BY` is a parameterless constant expression pushed as raw
    // SQL, while every value (`normalized_name`) goes through
    // `push_bind`. No user data ever reaches the raw SQL, so there's no
    // `AssertSqlSafe` audit hatch to rot if this query is edited later.
    let mut qb = sqlx::QueryBuilder::new(
        "SELECT id, normalized_name, season, episode, indexer_provider, indexer_torrent_id, \
                magnet, quality, seeders, size_bytes, found_at, language, download_url, \
                absolute_episode, codec \
         FROM (SELECT *, ROW_NUMBER() OVER ( \
                   PARTITION BY season, episode, COALESCE(language, '') \
                   ORDER BY ",
    );
    qb.push(recommended_order_sql());
    qb.push(
        ") AS _rn \
               FROM available_episodes \
               WHERE normalized_name = ",
    );
    qb.push_bind(normalized_name);
    qb.push(
        " AND episode > 0 AND seeders IS NOT 0) \
         WHERE _rn = 1 \
         ORDER BY season, episode, language",
    );
    qb.build_query_as::<AvailableEpisodeRow>()
        .fetch_all(pool)
        .await
}

/// Best-quality season-pack offers (episode == 0 sentinel) for a
/// series. The scheduler caches packs alongside individual episodes
/// so the grab path can fall back to "ingest the pack, find the
/// requested episode inside" when no singleton offer exists. The
/// API exposes packs in their own `season_packs` field — the UI
/// never renders them as episode rows.
///
/// Same dedup-by-language semantics as `list_best_for_series` — one row
/// per (season, language) so the household's anglophone + francophone
/// users can both find a coverable pack — but the winner per group is
/// the **recommended-best** (smallest sane pack, seeders as garde-fou,
/// `MULTi` discounted), picked in one SQL pass. A lighter `MULTi`/1080p
/// pack beats a 4K monster.
///
/// Packs with exactly **0 seeders are excluded** (`seeders IS NOT 0`):
/// they're un-grabbable, so they must never surface as a "Grab full
/// Season N" affordance nor be picked by the grab fallback. Unknown
/// (NULL) seeder counts are kept — we can't confirm those are dead.
///
/// Packs redundant with episode-level coverage the series already has are
/// also dropped (see [`pack_is_redundant`]): a `MULTi` episode release
/// satisfies both FR and EN, so a FR or EN pack adds nothing once one
/// exists; a French episode release already meets the household's language
/// need, so a `MULTi` (heavier, redundant audio) or another French pack
/// adds nothing either. English packs are never suppressed by French
/// coverage alone — only `MULTi`/French redundancy is filtered.
///
/// `owned_coverage` is per-season language coverage from what's actually in
/// the library (`episode_files`, keyed by season → the set of languages
/// already on disk for that season) — NOT from this module's own
/// `available_episodes` cache. A season grabbed as one Multi season-pack
/// torrent has no individual `episode > 0` rows in `available_episodes` at
/// all (the indexer only ever offered it as the pack the household grabbed,
/// or those offer rows since expired from the cache), so deriving coverage
/// from this table alone missed exactly the "already own the whole season
/// in Multi" case — the suppression never fired and a redundant FR/EN pack
/// kept showing. The caller (`library.rs`'s `build_tv_episode_view`) already
/// computes this map for the per-episode "available" filter; pass the same
/// one here instead of re-deriving a second, incomplete coverage signal.
#[allow(clippy::implicit_hasher)] // internal app, not a generic-hasher-consuming library
pub async fn list_season_packs_for_series(
    pool: &SqlitePool,
    normalized_name: &str,
    owned_coverage: &HashMap<i64, HashSet<String>>,
) -> Result<Vec<AvailableEpisodeRow>, sqlx::Error> {
    let mut qb = sqlx::QueryBuilder::new(
        "SELECT id, normalized_name, season, episode, indexer_provider, indexer_torrent_id, \
                magnet, quality, seeders, size_bytes, found_at, language, download_url, \
                absolute_episode, codec \
         FROM (SELECT *, ROW_NUMBER() OVER ( \
                   PARTITION BY season, COALESCE(language, '') \
                   ORDER BY ",
    );
    qb.push(recommended_order_sql());
    qb.push(
        ") AS _rn \
               FROM available_episodes \
               WHERE normalized_name = ",
    );
    qb.push_bind(normalized_name);
    qb.push(
        " AND episode = 0 AND seeders IS NOT 0) \
         WHERE _rn = 1 \
         ORDER BY season, language",
    );
    let packs = qb
        .build_query_as::<AvailableEpisodeRow>()
        .fetch_all(pool)
        .await?;

    Ok(packs
        .into_iter()
        .filter(|pack| !pack_is_redundant(pack, owned_coverage))
        .collect())
}

/// `true` when `pack`'s language is redundant with the episode-level
/// coverage already available for its season — see
/// [`list_season_packs_for_series`]'s doc comment for the exact rule.
fn pack_is_redundant(pack: &AvailableEpisodeRow, coverage: &HashMap<i64, HashSet<String>>) -> bool {
    let Some(langs) = coverage.get(&pack.season) else {
        return false;
    };
    let has_multi = langs.contains("multi");
    let has_fr = langs.contains("french");
    let pack_lang = pack.language.as_deref().unwrap_or("unknown");
    (has_multi && matches!(pack_lang, "french" | "english"))
        || (has_fr && matches!(pack_lang, "multi" | "french"))
}

/// Find the best season-pack offer for a specific `(normalized_name,
/// season)` — used by the grab path to ingest a pack covering an
/// episode that has no singleton offer. When `language_pref` is set we
/// honour it exactly (no cross-language fallback, same contract as the
/// singleton grab); otherwise we take the **recommended-best** pack
/// across languages. Either way the ordering is the shared size-first /
/// seeders-garde-fou / `MULTi`-discounted policy, in one SQL pass, so we
/// never auto-ingest a 51 GB 4K season.
pub async fn find_pack_for_season(
    pool: &SqlitePool,
    normalized_name: &str,
    season: i64,
    language_pref: Option<&str>,
) -> Result<Option<AvailableEpisodeRow>, sqlx::Error> {
    // `language_pref` is bound twice (the original `?3` appeared twice as
    // `?3 IS NULL OR language = ?3`); QueryBuilder placeholders are
    // positional, so each `push_bind` emits its own `?`.
    let mut qb = sqlx::QueryBuilder::new(
        "SELECT id, normalized_name, season, episode, indexer_provider, indexer_torrent_id, \
                magnet, quality, seeders, size_bytes, found_at, language, download_url, \
                absolute_episode, codec \
         FROM available_episodes \
         WHERE normalized_name = ",
    );
    qb.push_bind(normalized_name);
    qb.push(" AND episode = 0 AND season = ");
    qb.push_bind(season);
    qb.push(" AND seeders IS NOT 0 AND (");
    qb.push_bind(language_pref);
    qb.push(" IS NULL OR language = ");
    qb.push_bind(language_pref);
    qb.push(") ORDER BY ");
    qb.push(recommended_order_sql());
    qb.push(" LIMIT 1");
    qb.build_query_as::<AvailableEpisodeRow>()
        .fetch_optional(pool)
        .await
}

/// Every live cached offer for one exact `(season, episode)` — raw rows,
/// no per-language dedup. The grab path re-ranks them in Rust with the
/// series' established format / codec profile (which SQL can't know), so
/// it needs the full candidate set, not the display-side winner.
/// Confirmed-dead offers (exactly 0 seeders) are excluded — a grab must
/// never ingest a torrent whose pieces can't assemble; NULL seeder
/// counts pass (can't confirm those are dead).
pub async fn list_offers_for_episode(
    pool: &SqlitePool,
    normalized_name: &str,
    season: i64,
    episode: i64,
) -> Result<Vec<AvailableEpisodeRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, normalized_name, season, episode, indexer_provider, indexer_torrent_id, \
                magnet, quality, seeders, size_bytes, found_at, language, download_url, \
                absolute_episode, codec \
         FROM available_episodes \
         WHERE normalized_name = ?1 AND season = ?2 AND episode = ?3 \
           AND episode > 0 AND seeders IS NOT 0",
    )
    .bind(normalized_name)
    .bind(season)
    .bind(episode)
    .fetch_all(pool)
    .await
}

/// Every live cached season-pack offer (`episode == 0` sentinel) for one
/// season — raw rows for the same Rust-side profile-aware ranking as
/// [`list_offers_for_episode`]. Same 0-seeder exclusion.
pub async fn list_pack_offers_for_season(
    pool: &SqlitePool,
    normalized_name: &str,
    season: i64,
) -> Result<Vec<AvailableEpisodeRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, normalized_name, season, episode, indexer_provider, indexer_torrent_id, \
                magnet, quality, seeders, size_bytes, found_at, language, download_url, \
                absolute_episode, codec \
         FROM available_episodes \
         WHERE normalized_name = ?1 AND season = ?2 AND episode = 0 AND seeders IS NOT 0",
    )
    .bind(normalized_name)
    .bind(season)
    .fetch_all(pool)
    .await
}

/// Overwrite a cached offer's seeder count with a fresh number from a
/// live indexer sweep. Recording a confirmed 0 is the point: every
/// reader filters `seeders IS NOT 0`, so the dead offer vanishes from
/// grabs and availability badges alike until a future scan sees it
/// alive again.
pub async fn set_seeders(pool: &SqlitePool, id: Uuid, seeders: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE available_episodes SET seeders = ?2 WHERE id = ?1")
        .bind(id)
        .bind(seeders)
        .execute(pool)
        .await?;
    Ok(())
}

/// Distinct episodes found after `since` — the "X new" Watchlist
/// badge. `since` = the user's last engagement (max of page visit and
/// watch). No engagement → 0, NOT "everything": counting the whole
/// offer cache produced "48 new" badges.
pub async fn count_new_for_series(
    pool: &SqlitePool,
    normalized_name: &str,
    since: Option<DateTime<Utc>>,
) -> Result<i64, sqlx::Error> {
    let Some(cutoff) = since else {
        return Ok(0);
    };
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT season || '-' || episode) FROM available_episodes \
         WHERE normalized_name = ?1 AND found_at > ?2",
    )
    .bind(normalized_name)
    .bind(cutoff)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(season: i64, language: Option<&str>) -> AvailableEpisodeRow {
        AvailableEpisodeRow {
            id: Uuid::new_v4(),
            normalized_name: "test".to_string(),
            season,
            episode: 0,
            indexer_provider: "test".to_string(),
            indexer_torrent_id: "1".to_string(),
            magnet: "magnet:?xt=urn:btih:test".to_string(),
            quality: None,
            seeders: Some(5),
            size_bytes: None,
            found_at: Utc::now(),
            language: language.map(str::to_string),
            download_url: None,
            absolute_episode: None,
            codec: None,
        }
    }

    fn coverage(season: i64, langs: &[&str]) -> HashMap<i64, HashSet<String>> {
        let mut m = HashMap::new();
        m.insert(season, langs.iter().map(ToString::to_string).collect());
        m
    }

    #[test]
    fn no_coverage_never_redundant() {
        let p = pack(1, Some("french"));
        assert!(!pack_is_redundant(&p, &HashMap::new()));
    }

    #[test]
    fn multi_coverage_suppresses_french_and_english_packs() {
        let cov = coverage(1, &["multi"]);
        assert!(pack_is_redundant(&pack(1, Some("french")), &cov));
        assert!(pack_is_redundant(&pack(1, Some("english")), &cov));
        // A fresh Multi pack itself is still worth surfacing (e.g. better
        // quality/seeders than the episode-level offers).
        assert!(!pack_is_redundant(&pack(1, Some("multi")), &cov));
    }

    #[test]
    fn french_coverage_suppresses_multi_and_french_packs_but_not_english() {
        let cov = coverage(1, &["french"]);
        assert!(pack_is_redundant(&pack(1, Some("multi")), &cov));
        assert!(pack_is_redundant(&pack(1, Some("french")), &cov));
        assert!(!pack_is_redundant(&pack(1, Some("english")), &cov));
    }

    #[test]
    fn unknown_language_pack_is_never_suppressed() {
        let cov = coverage(1, &["multi"]);
        assert!(!pack_is_redundant(&pack(1, None), &cov));
    }

    #[test]
    fn coverage_is_scoped_per_season() {
        let cov = coverage(1, &["multi"]);
        // Season 2 has no recorded coverage, so its packs are untouched.
        assert!(!pack_is_redundant(&pack(2, Some("french")), &cov));
    }
}

#[derive(Debug, Clone)]
pub struct UpsertAvailableEpisode {
    pub normalized_name: String,
    pub season: i64,
    pub episode: i64,
    pub indexer_provider: String,
    pub indexer_torrent_id: String,
    pub magnet: String,
    pub quality: Option<String>,
    pub seeders: Option<i64>,
    pub size_bytes: Option<i64>,
    /// `"french"` / `"english"` / `"multi"` / `"unknown"` — the
    /// `Display` form of `iris_media::filename::Language`. `None`
    /// is only emitted by legacy code paths that pre-date the
    /// multi-language work; new writes always set it.
    pub language: Option<String>,
    /// Pre-signed `.torrent` download URL captured at scan time
    /// from Torznab `<link>` / UNIT3D `download_link`. Lets the
    /// grab path stay alive across server restarts that wipe the
    /// providers' in-memory link caches.
    pub download_url: Option<String>,
    /// Absolute episode number for fleuve anime offers (`None` for
    /// seasonal releases and packs).
    pub absolute_episode: Option<i64>,
    /// Coarse video codec parsed from the release title —
    /// `iris_media::filename::Codec::as_str` form. `None` only from
    /// legacy code paths pre-dating the format-follow work.
    pub codec: Option<String>,
}

pub async fn upsert(pool: &SqlitePool, a: UpsertAvailableEpisode) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO available_episodes \
            (id, normalized_name, season, episode, indexer_provider, indexer_torrent_id, \
             magnet, quality, seeders, size_bytes, found_at, language, download_url, \
             absolute_episode, codec) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15) \
         ON CONFLICT(normalized_name, season, episode, indexer_provider, indexer_torrent_id) \
         DO UPDATE SET \
            magnet           = excluded.magnet, \
            quality          = excluded.quality, \
            seeders          = excluded.seeders, \
            size_bytes       = excluded.size_bytes, \
            language         = excluded.language, \
            download_url     = excluded.download_url, \
            absolute_episode = excluded.absolute_episode, \
            codec            = excluded.codec",
    )
    .bind(Uuid::new_v4())
    .bind(&a.normalized_name)
    .bind(a.season)
    .bind(a.episode)
    .bind(&a.indexer_provider)
    .bind(&a.indexer_torrent_id)
    .bind(&a.magnet)
    .bind(a.quality)
    .bind(a.seeders)
    .bind(a.size_bytes)
    .bind(Utc::now())
    .bind(a.language)
    .bind(a.download_url)
    .bind(a.absolute_episode)
    .bind(a.codec)
    .execute(pool)
    .await?;
    Ok(())
}

/// Hard-delete every cached offer for a normalised series name. Used by
/// the boot self-heal after it splits a mixed collection (e.g. the
/// anime + live-action `"one piece"` amalgam): the stale rows recorded
/// under the old shared name would otherwise keep surfacing on the
/// post-split collections. The scheduler re-records correctly-keyed
/// offers on its next scan. Returns the number of rows removed.
pub async fn delete_for_series(
    pool: &SqlitePool,
    normalized_name: &str,
) -> Result<u64, sqlx::Error> {
    let res = sqlx::query("DELETE FROM available_episodes WHERE normalized_name = ?1")
        .bind(normalized_name)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}
