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
pub async fn list_best_for_series(
    pool: &SqlitePool,
    normalized_name: &str,
) -> Result<Vec<AvailableEpisodeRow>, sqlx::Error> {
    let sql = format!(
        "SELECT id, normalized_name, season, episode, indexer_provider, indexer_torrent_id, \
                magnet, quality, seeders, size_bytes, found_at, language, download_url \
         FROM (SELECT *, ROW_NUMBER() OVER ( \
                   PARTITION BY season, episode, COALESCE(language, '') \
                   ORDER BY {order}) AS _rn \
               FROM available_episodes \
               WHERE normalized_name = ?1 AND episode > 0) \
         WHERE _rn = 1 \
         ORDER BY season, episode, language",
        order = recommended_order_sql(),
    );
    sqlx::query_as::<_, AvailableEpisodeRow>(&sql)
        .bind(normalized_name)
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
pub async fn list_season_packs_for_series(
    pool: &SqlitePool,
    normalized_name: &str,
) -> Result<Vec<AvailableEpisodeRow>, sqlx::Error> {
    let sql = format!(
        "SELECT id, normalized_name, season, episode, indexer_provider, indexer_torrent_id, \
                magnet, quality, seeders, size_bytes, found_at, language, download_url \
         FROM (SELECT *, ROW_NUMBER() OVER ( \
                   PARTITION BY season, COALESCE(language, '') \
                   ORDER BY {order}) AS _rn \
               FROM available_episodes \
               WHERE normalized_name = ?1 AND episode = 0 AND seeders IS NOT 0) \
         WHERE _rn = 1 \
         ORDER BY season, language",
        order = recommended_order_sql(),
    );
    sqlx::query_as::<_, AvailableEpisodeRow>(&sql)
        .bind(normalized_name)
        .fetch_all(pool)
        .await
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
    let sql = format!(
        "SELECT id, normalized_name, season, episode, indexer_provider, indexer_torrent_id, \
                magnet, quality, seeders, size_bytes, found_at, language, download_url \
         FROM available_episodes \
         WHERE normalized_name = ?1 AND episode = 0 AND season = ?2 AND seeders IS NOT 0 \
           AND (?3 IS NULL OR language = ?3) \
         ORDER BY {order} \
         LIMIT 1",
        order = recommended_order_sql(),
    );
    sqlx::query_as::<_, AvailableEpisodeRow>(&sql)
        .bind(normalized_name)
        .bind(season)
        .bind(language_pref)
        .fetch_optional(pool)
        .await
}

/// Count of distinct episodes whose `found_at` is newer than
/// `since`. Drives the "X nouveaux" badge on Watchlist cards.
pub async fn count_new_for_series(
    pool: &SqlitePool,
    normalized_name: &str,
    since: Option<DateTime<Utc>>,
) -> Result<i64, sqlx::Error> {
    let cutoff = since.unwrap_or_else(|| {
        // No prior visit means "everything currently available is
        // new". Old enough timestamp to catch the lot.
        DateTime::<Utc>::from_timestamp(0, 0).unwrap()
    });
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
}

pub async fn upsert(
    pool: &SqlitePool,
    a: UpsertAvailableEpisode,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO available_episodes \
            (id, normalized_name, season, episode, indexer_provider, indexer_torrent_id, \
             magnet, quality, seeders, size_bytes, found_at, language, download_url) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13) \
         ON CONFLICT(normalized_name, season, episode, indexer_provider, indexer_torrent_id) \
         DO UPDATE SET \
            magnet       = excluded.magnet, \
            quality      = excluded.quality, \
            seeders      = excluded.seeders, \
            size_bytes   = excluded.size_bytes, \
            language     = excluded.language, \
            download_url = excluded.download_url",
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
    .execute(pool)
    .await?;
    Ok(())
}
