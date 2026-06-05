//! Per-user playback preferences: preferred audio + subtitle *language*.
//!
//! Unlike `playback_progress` (per-file track *indices*), these are
//! language-keyed and apply across files, so a user's "French audio, English
//! subs" choice carries to the next episode and to any device. Applied at
//! playback time by matching the file's tracks; missing languages fall back
//! gracefully (handled client-side). See migration 0024 for the layering.

use chrono::Utc;
use iris_core::ids::UserId;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

/// A user's playback language preferences. Both `None` = "no preference yet"
/// (cold start). `subtitle_language == Some("off")` means subtitles disabled.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlaybackPreferences {
    pub audio_language: Option<String>,
    pub subtitle_language: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct PrefRow {
    audio_language: Option<String>,
    subtitle_language: Option<String>,
}

/// Resolve a user's playback preferences, returning the all-`None` default
/// when no row exists yet (never set a track preference).
pub async fn get(pool: &SqlitePool, user_id: UserId) -> Result<PlaybackPreferences, sqlx::Error> {
    let uuid: Uuid = user_id.into();
    let row: Option<PrefRow> = sqlx::query_as(
        "SELECT audio_language, subtitle_language FROM playback_preferences WHERE user_id = ?1",
    )
    .bind(uuid)
    .fetch_optional(pool)
    .await?;
    Ok(row
        .map(|r| PlaybackPreferences {
            audio_language: r.audio_language,
            subtitle_language: r.subtitle_language,
        })
        .unwrap_or_default())
}

/// Insert-or-replace a user's playback preferences. The client sends the full
/// current state (both fields), so a plain replace is correct here — and safe,
/// because only the dedicated playback-prefs client writes this row.
pub async fn set(
    pool: &SqlitePool,
    user_id: UserId,
    prefs: &PlaybackPreferences,
) -> Result<(), sqlx::Error> {
    let uuid: Uuid = user_id.into();
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO playback_preferences (user_id, audio_language, subtitle_language, updated_at) \
         VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(user_id) DO UPDATE SET \
            audio_language = excluded.audio_language, \
            subtitle_language = excluded.subtitle_language, \
            updated_at = excluded.updated_at",
    )
    .bind(uuid)
    .bind(&prefs.audio_language)
    .bind(&prefs.subtitle_language)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn migrated_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        crate::migrate::run(&pool).await.expect("run migrations");
        pool
    }

    async fn make_user(pool: &SqlitePool) -> UserId {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO users (id, email, password_hash, display_name, is_admin, created_at) \
             VALUES (?1, ?2, '', 'T', 0, ?3)",
        )
        .bind(id)
        .bind(format!("{id}@t.test"))
        .bind(Utc::now())
        .execute(pool)
        .await
        .expect("insert user");
        UserId::from(id)
    }

    #[tokio::test]
    async fn defaults_then_roundtrips() {
        let pool = migrated_pool().await;
        let user = make_user(&pool).await;

        // Cold start → all None.
        let p = get(&pool, user).await.unwrap();
        assert!(p.audio_language.is_none() && p.subtitle_language.is_none());

        // Set + read back (incl. the 'off' subtitle sentinel).
        set(
            &pool,
            user,
            &PlaybackPreferences {
                audio_language: Some("fr".to_string()),
                subtitle_language: Some("off".to_string()),
            },
        )
        .await
        .unwrap();
        let p = get(&pool, user).await.unwrap();
        assert_eq!(p.audio_language.as_deref(), Some("fr"));
        assert_eq!(p.subtitle_language.as_deref(), Some("off"));

        // Replace (upsert) overwrites in place.
        set(
            &pool,
            user,
            &PlaybackPreferences {
                audio_language: Some("en".to_string()),
                subtitle_language: Some("fr".to_string()),
            },
        )
        .await
        .unwrap();
        let p = get(&pool, user).await.unwrap();
        assert_eq!(p.audio_language.as_deref(), Some("en"));
        assert_eq!(p.subtitle_language.as_deref(), Some("fr"));
    }
}
