//! `users.sessions_valid_after`, read on every authenticated request: kept in
//! memory, loaded from the DB on first sight of a user (so a restart is
//! correct) and dropped by every write that moves it.

use std::collections::HashMap;
use std::sync::{PoisonError, RwLock};

use iris_core::ids::UserId;
use sqlx::SqlitePool;

use crate::error::ApiError;

#[derive(Default)]
pub struct SessionCuts {
    inner: RwLock<Cuts>,
}

#[derive(Default)]
struct Cuts {
    /// Per user: the cut in milliseconds, `None` when it was never stamped.
    by_user: HashMap<UserId, Option<i64>>,
    /// Bumped by every [`SessionCuts::forget`]: a read that started before one
    /// may have seen the row before the write and must not be cached.
    generation: u64,
}

impl SessionCuts {
    /// `Unauthorized` when the user is gone or the token predates their cut.
    pub async fn check(
        &self,
        pool: &SqlitePool,
        user: UserId,
        issued_at_ms: i64,
    ) -> Result<(), ApiError> {
        let (cached, generation) = {
            let cuts = self.inner.read().unwrap_or_else(PoisonError::into_inner);
            (cuts.by_user.get(&user).copied(), cuts.generation)
        };
        let cut = if let Some(cut) = cached {
            cut
        } else {
            let cut = iris_db::users::sessions_valid_after(pool, user)
                .await?
                .ok_or(ApiError::Unauthorized)?
                .map(|t| t.timestamp_millis());
            let mut cuts = self.inner.write().unwrap_or_else(PoisonError::into_inner);
            if cuts.generation == generation {
                cuts.by_user.insert(user, cut);
            }
            cut
        };
        if cut.is_some_and(|cut| issued_at_ms < cut) {
            return Err(ApiError::Unauthorized);
        }
        Ok(())
    }

    /// After a committed write to the user's row (cut stamped, account
    /// deleted): the next request reads it again.
    pub fn forget(&self, user: UserId) {
        let mut cuts = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        cuts.by_user.remove(&user);
        cuts.generation += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::SessionCuts;
    use crate::error::ApiError;

    #[tokio::test]
    async fn a_token_older_than_the_cut_is_refused_once_the_cache_forgets() {
        let pool = iris_db::test_support::migrated_pool().await;
        let user = iris_db::test_support::make_user(&pool).await;
        let cuts = SessionCuts::default();
        let before = chrono::Utc::now().timestamp_millis() - 1;
        cuts.check(&pool, user, before).await.unwrap();

        iris_db::users::set_password(&pool, user, "h")
            .await
            .unwrap();
        cuts.forget(user);
        assert!(matches!(
            cuts.check(&pool, user, before).await,
            Err(ApiError::Unauthorized)
        ));
        let after = chrono::Utc::now().timestamp_millis();
        cuts.check(&pool, user, after).await.unwrap();

        let ghost = iris_core::ids::UserId::from(uuid::Uuid::new_v4());
        assert!(matches!(
            cuts.check(&pool, ghost, after).await,
            Err(ApiError::Unauthorized)
        ));
    }
}
