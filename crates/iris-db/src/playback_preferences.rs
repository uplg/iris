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

/// One title's preferences, field by field: what its own row holds, and that
/// laid over the account-wide ones (a NULL field inherits the account value).
#[derive(Debug, Clone, Default)]
pub struct CollectionPreferences {
    pub merged: PlaybackPreferences,
    pub own: PlaybackPreferences,
}

impl CollectionPreferences {
    /// The title holds a choice of its own for at least one field.
    #[must_use]
    pub fn for_collection(&self) -> bool {
        self.own.audio_language.is_some() || self.own.subtitle_language.is_some()
    }
}

/// The preferences for one title (series or film): per field, its own choice
/// when it made one, else the account-wide one.
pub async fn get_for_collection(
    pool: &SqlitePool,
    user_id: UserId,
    collection_id: Uuid,
) -> Result<CollectionPreferences, sqlx::Error> {
    let uuid: Uuid = user_id.into();
    let row: Option<PrefRow> = sqlx::query_as(
        "SELECT audio_language, subtitle_language FROM collection_playback_preferences \
         WHERE user_id = ?1 AND collection_id = ?2",
    )
    .bind(uuid)
    .bind(collection_id)
    .fetch_optional(pool)
    .await?;
    let own = row
        .map(|r| PlaybackPreferences {
            audio_language: r.audio_language,
            subtitle_language: r.subtitle_language,
        })
        .unwrap_or_default();
    let account = get(pool, user_id).await?;
    Ok(CollectionPreferences {
        merged: PlaybackPreferences {
            audio_language: own.audio_language.clone().or(account.audio_language),
            subtitle_language: own.subtitle_language.clone().or(account.subtitle_language),
        },
        own,
    })
}

/// Insert-or-replace one title's own choices. A `None` field is no choice of
/// its own: it inherits the account-wide value at read time.
pub async fn set_for_collection(
    pool: &SqlitePool,
    user_id: UserId,
    collection_id: Uuid,
    prefs: &PlaybackPreferences,
) -> Result<(), sqlx::Error> {
    let uuid: Uuid = user_id.into();
    sqlx::query(
        "INSERT INTO collection_playback_preferences \
           (user_id, collection_id, audio_language, subtitle_language, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT(user_id, collection_id) DO UPDATE SET \
            audio_language = excluded.audio_language, \
            subtitle_language = excluded.subtitle_language, \
            updated_at = excluded.updated_at",
    )
    .bind(uuid)
    .bind(collection_id)
    .bind(&prefs.audio_language)
    .bind(&prefs.subtitle_language)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{make_user, migrated_pool};

    async fn make_collection(pool: &SqlitePool, name: &str) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO collections (id, parsed_title_normalized, display_title, kind, created_at) \
             VALUES (?1, ?2, ?2, 'tv', ?3)",
        )
        .bind(id)
        .bind(name)
        .bind(Utc::now())
        .execute(pool)
        .await
        .expect("insert collection");
        id
    }

    #[tokio::test]
    async fn a_series_choice_overrides_the_account_one() {
        let pool = migrated_pool().await;
        let user = make_user(&pool).await;
        let series = make_collection(&pool, "severance").await;
        let account = PlaybackPreferences {
            audio_language: Some("fr".into()),
            subtitle_language: Some("off".into()),
        };
        set(&pool, user, &account).await.unwrap();
        let p = get_for_collection(&pool, user, series).await.unwrap();
        assert!(!p.for_collection());
        assert_eq!(p.merged.audio_language.as_deref(), Some("fr"));

        let mine = PlaybackPreferences {
            audio_language: Some("en".into()),
            subtitle_language: Some("en".into()),
        };
        set_for_collection(&pool, user, series, &mine)
            .await
            .unwrap();
        let p = get_for_collection(&pool, user, series).await.unwrap();
        assert!(p.for_collection());
        assert_eq!(p.merged.audio_language.as_deref(), Some("en"));
        // The account-wide choice is untouched.
        assert_eq!(
            get(&pool, user).await.unwrap().audio_language.as_deref(),
            Some("fr")
        );
    }

    #[tokio::test]
    async fn an_unset_field_inherits_the_account_value() {
        let pool = migrated_pool().await;
        let user = make_user(&pool).await;
        let film = make_collection(&pool, "avatar").await;
        set(
            &pool,
            user,
            &PlaybackPreferences {
                audio_language: Some("kor".into()),
                subtitle_language: Some("fre".into()),
            },
        )
        .await
        .unwrap();
        let subs_off = PlaybackPreferences {
            audio_language: None,
            subtitle_language: Some("off".into()),
        };
        set_for_collection(&pool, user, film, &subs_off)
            .await
            .unwrap();
        let p = get_for_collection(&pool, user, film).await.unwrap();
        assert!(p.for_collection());
        assert_eq!(p.merged.audio_language.as_deref(), Some("kor"));
        assert_eq!(p.merged.subtitle_language.as_deref(), Some("off"));
        assert!(p.own.audio_language.is_none());

        // The account default moves: the title follows it for the field it never chose.
        set(
            &pool,
            user,
            &PlaybackPreferences {
                audio_language: Some("fre".into()),
                subtitle_language: Some("eng".into()),
            },
        )
        .await
        .unwrap();
        let p = get_for_collection(&pool, user, film).await.unwrap();
        assert_eq!(p.merged.audio_language.as_deref(), Some("fre"));
        assert_eq!(p.merged.subtitle_language.as_deref(), Some("off"));
    }

    #[tokio::test]
    async fn a_row_with_no_choice_is_not_the_titles_own() {
        let pool = migrated_pool().await;
        let user = make_user(&pool).await;
        let film = make_collection(&pool, "heat").await;
        set_for_collection(&pool, user, film, &PlaybackPreferences::default())
            .await
            .unwrap();
        assert!(
            !get_for_collection(&pool, user, film)
                .await
                .unwrap()
                .for_collection()
        );
    }

    /// Migration 0046: a field the clients copied from the account default goes
    /// back to inheriting it; a real choice stays.
    #[tokio::test]
    async fn copied_account_values_are_cleared() {
        let pool = migrated_pool().await;
        let user = make_user(&pool).await;
        let other = make_user(&pool).await;
        let avatar = make_collection(&pool, "avatar").await;
        let copied = make_collection(&pool, "copied").await;
        let account = PlaybackPreferences {
            audio_language: Some("kor".into()),
            subtitle_language: Some("fre".into()),
        };
        set(&pool, user, &account).await.unwrap();
        let avatar_row = PlaybackPreferences {
            audio_language: Some("kor".into()),
            subtitle_language: Some("off".into()),
        };
        set_for_collection(&pool, user, avatar, &avatar_row)
            .await
            .unwrap();
        set_for_collection(&pool, user, copied, &account)
            .await
            .unwrap();
        // No account row: nothing to compare with, the choice stays.
        set_for_collection(&pool, other, avatar, &account)
            .await
            .unwrap();

        sqlx::raw_sql(include_str!(
            "../../../migrations/0046_collection_prefs_inherit.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();

        let p = get_for_collection(&pool, user, avatar).await.unwrap();
        assert!(p.own.audio_language.is_none());
        assert_eq!(p.own.subtitle_language.as_deref(), Some("off"));
        assert_eq!(p.merged.audio_language.as_deref(), Some("kor"));
        assert!(
            !get_for_collection(&pool, user, copied)
                .await
                .unwrap()
                .for_collection()
        );
        let rows: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM collection_playback_preferences WHERE collection_id = ?1",
        )
        .bind(copied)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(rows.0, 0);
        let p = get_for_collection(&pool, other, avatar).await.unwrap();
        assert_eq!(p.own.audio_language.as_deref(), Some("kor"));
        assert_eq!(p.own.subtitle_language.as_deref(), Some("fre"));
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
