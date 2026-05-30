//! Per-user recommendation preferences (languages, genres, anime
//! toggle, onboarding state). Backs the onboarding dialog and the
//! reco scheduler's "what slices does anyone actually want" union.
//!
//! `languages` / `genres` are stored as JSON text columns and surfaced
//! as typed `Vec`s here — we own every write, so a malformed blob is a
//! bug, not a runtime path: reads fall back to empty rather than error.

use chrono::{DateTime, Utc};
use iris_core::ids::UserId;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

/// A user's resolved recommendation preferences. `languages` is ordered
/// most-preferred first using the `iris_media::filename::Language`
/// vocabulary ("french" / "english"); `genres` holds TMDB genre ids.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserPreferences {
    pub languages: Vec<String>,
    pub genres: Vec<i64>,
    pub include_anime: bool,
    pub onboarding_completed: bool,
    /// `None` when the user has never saved preferences (the `get`
    /// fallback default).
    pub updated_at: Option<DateTime<Utc>>,
}

/// The mutable subset accepted from clients. `updated_at` / `created_at`
/// are stamped server-side; identity comes from the authenticated user.
#[derive(Debug, Clone)]
pub struct PreferencesUpdate {
    pub languages: Vec<String>,
    pub genres: Vec<i64>,
    pub include_anime: bool,
    pub onboarding_completed: bool,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct PrefRow {
    languages: String,
    genres: String,
    include_anime: bool,
    onboarding_completed: bool,
    updated_at: DateTime<Utc>,
}

impl PrefRow {
    fn into_domain(self) -> UserPreferences {
        UserPreferences {
            // We control every write, so these always parse; defaulting
            // to empty keeps a hand-edited DB from 500-ing the API.
            languages: serde_json::from_str(&self.languages).unwrap_or_default(),
            genres: serde_json::from_str(&self.genres).unwrap_or_default(),
            include_anime: self.include_anime,
            onboarding_completed: self.onboarding_completed,
            updated_at: Some(self.updated_at),
        }
    }
}

const SELECT_COLUMNS: &str =
    "languages, genres, include_anime, onboarding_completed, updated_at";

/// Resolve a user's preferences, returning the all-empty default when no
/// row exists yet (never-onboarded user). Callers treat the default as
/// "cold start" rather than an error.
pub async fn get(pool: &SqlitePool, user_id: UserId) -> Result<UserPreferences, sqlx::Error> {
    let uuid: Uuid = user_id.into();
    let q = format!("SELECT {SELECT_COLUMNS} FROM user_preferences WHERE user_id = ?1");
    let row: Option<PrefRow> = sqlx::query_as(&q)
        .bind(uuid)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(PrefRow::into_domain).unwrap_or_default())
}

/// Insert-or-replace a user's preferences and return the stored value.
pub async fn upsert(
    pool: &SqlitePool,
    user_id: UserId,
    update: &PreferencesUpdate,
) -> Result<UserPreferences, sqlx::Error> {
    let uuid: Uuid = user_id.into();
    let now = Utc::now();
    let languages = serde_json::to_string(&update.languages).unwrap_or_else(|_| "[]".to_string());
    let genres = serde_json::to_string(&update.genres).unwrap_or_else(|_| "[]".to_string());
    sqlx::query(
        "INSERT INTO user_preferences \
            (user_id, languages, genres, include_anime, onboarding_completed, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6) \
         ON CONFLICT(user_id) DO UPDATE SET \
            languages = excluded.languages, \
            genres = excluded.genres, \
            include_anime = excluded.include_anime, \
            onboarding_completed = excluded.onboarding_completed, \
            updated_at = excluded.updated_at",
    )
    .bind(uuid)
    .bind(&languages)
    .bind(&genres)
    .bind(update.include_anime)
    .bind(update.onboarding_completed)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(UserPreferences {
        languages: update.languages.clone(),
        genres: update.genres.clone(),
        include_anime: update.include_anime,
        onboarding_completed: update.onboarding_completed,
        updated_at: Some(now),
    })
}

/// Cheap "has this user finished onboarding?" probe. A missing row reads
/// as not-onboarded.
pub async fn is_onboarded(pool: &SqlitePool, user_id: UserId) -> Result<bool, sqlx::Error> {
    let uuid: Uuid = user_id.into();
    let row: Option<(bool,)> =
        sqlx::query_as("SELECT onboarding_completed FROM user_preferences WHERE user_id = ?1")
            .bind(uuid)
            .fetch_optional(pool)
            .await?;
    Ok(row.is_some_and(|r| r.0))
}

/// Every onboarded user's preferences. The reco scheduler unions these
/// (languages × genres × anime) to decide which TMDB slices to fetch,
/// so it batches by household demand instead of fetching per-user.
pub async fn all_onboarded(pool: &SqlitePool) -> Result<Vec<UserPreferences>, sqlx::Error> {
    let q = format!(
        "SELECT {SELECT_COLUMNS} FROM user_preferences WHERE onboarding_completed = 1"
    );
    let rows: Vec<PrefRow> = sqlx::query_as(&q).fetch_all(pool).await?;
    Ok(rows.into_iter().map(PrefRow::into_domain).collect())
}
