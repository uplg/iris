use chrono::{DateTime, Duration, Utc};
use iris_core::ids::UserId;
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RefreshToken {
    pub jti: Uuid,
    pub user_id: Uuid,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub device_label: Option<String>,
    pub device_kind: Option<String>,
}

/// A session token to store. `issued_at` / `expires_at` are the very instants
/// its JWT carries, so the JWT can be minted again from the row.
#[derive(Debug, Clone)]
pub struct NewSession<'a> {
    pub jti: Uuid,
    /// The first token's jti for a new session, the predecessor's family on
    /// a rotation.
    pub family_id: Uuid,
    pub user_id: UserId,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub device_label: Option<&'a str>,
    pub device_kind: Option<&'a str>,
}

/// A stored, live session token: what a refresh hands back.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Session {
    pub jti: Uuid,
    pub user_id: Uuid,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub device_label: Option<String>,
    pub device_kind: Option<String>,
}

/// The token a rotation mints, chosen by the caller from the predecessor's
/// device kind (paired devices get a longer TTL).
#[derive(Debug, Clone, Copy)]
pub struct Successor {
    pub jti: Uuid,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rotation {
    /// The token was live: it is retired and this successor replaces it.
    Rotated(Session),
    /// The token was rotated within the grace window (several tabs refreshing
    /// at once, a retried request): the successor already minted for it.
    Replayed(Session),
    /// Rotated longer ago than the grace window: a stolen token replayed, or
    /// one kept around. The whole family was revoked.
    FamilyRevoked,
    /// Unknown, expired or revoked.
    Rejected,
}

pub async fn insert_session<'e, E>(executor: E, new: &NewSession<'_>) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let user: Uuid = new.user_id.into();
    sqlx::query(
        "INSERT INTO refresh_tokens \
         (jti, family_id, user_id, issued_at, expires_at, device_label, device_kind) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )
    .bind(new.jti)
    .bind(new.family_id)
    .bind(user)
    .bind(new.issued_at)
    .bind(new.expires_at)
    .bind(new.device_label)
    .bind(new.device_kind)
    .execute(executor)
    .await?;
    Ok(())
}

pub async fn insert(
    pool: &SqlitePool,
    jti: Uuid,
    user_id: UserId,
    expires_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    insert_with_device(pool, jti, user_id, expires_at, None, None).await
}

/// A new session of its own family, issued now.
pub async fn insert_with_device(
    pool: &SqlitePool,
    jti: Uuid,
    user_id: UserId,
    expires_at: DateTime<Utc>,
    device_label: Option<&str>,
    device_kind: Option<&str>,
) -> Result<(), sqlx::Error> {
    insert_session(
        pool,
        &NewSession {
            jti,
            family_id: jti,
            user_id,
            issued_at: Utc::now(),
            expires_at,
            device_label,
            device_kind,
        },
    )
    .await
}

pub async fn list_devices_for_user(
    pool: &SqlitePool,
    user_id: UserId,
) -> Result<Vec<RefreshToken>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_as::<_, RefreshToken>(
        "SELECT jti, user_id, issued_at, expires_at, revoked_at, device_label, device_kind \
         FROM refresh_tokens \
         WHERE user_id = ?1 AND revoked_at IS NULL AND device_kind IS NOT NULL \
           AND expires_at > ?2 \
         ORDER BY issued_at DESC",
    )
    .bind(user)
    .bind(Utc::now())
    .fetch_all(pool)
    .await
}

/// End the session `jti` belongs to, every token of its family, when it is
/// one of `user_id`'s. `false`: no such session for this user.
pub async fn revoke_for_user(
    pool: &SqlitePool,
    user_id: UserId,
    jti: Uuid,
) -> Result<bool, sqlx::Error> {
    let user: Uuid = user_id.into();
    let res = sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = COALESCE(revoked_at, ?1), rotated_at = NULL \
         WHERE user_id = ?3 AND family_id = \
           (SELECT family_id FROM refresh_tokens \
            WHERE jti = ?2 AND user_id = ?3 AND revoked_at IS NULL)",
    )
    .bind(Utc::now())
    .bind(jti)
    .bind(user)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn is_active(pool: &SqlitePool, jti: Uuid) -> Result<bool, sqlx::Error> {
    let row: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM refresh_tokens \
         WHERE jti = ?1 AND revoked_at IS NULL AND expires_at > ?2",
    )
    .bind(jti)
    .bind(Utc::now())
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

/// End the session `jti` belongs to (logout): every token of its family,
/// rotated ones included, so none comes back through the grace window.
pub async fn revoke<'e, E>(executor: E, jti: Uuid) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = COALESCE(revoked_at, ?1), rotated_at = NULL \
         WHERE family_id = (SELECT family_id FROM refresh_tokens WHERE jti = ?2)",
    )
    .bind(Utc::now())
    .bind(jti)
    .execute(executor)
    .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct ChainRow {
    #[sqlx(flatten)]
    session: Session,
    family_id: Uuid,
    revoked_at: Option<DateTime<Utc>>,
    rotated_at: Option<DateTime<Utc>>,
    successor_jti: Option<Uuid>,
}

async fn chain_row(
    conn: &mut sqlx::SqliteConnection,
    jti: Uuid,
) -> Result<Option<ChainRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT jti, user_id, issued_at, expires_at, device_label, device_kind, \
                COALESCE(family_id, jti) AS family_id, revoked_at, rotated_at, successor_jti \
         FROM refresh_tokens WHERE jti = ?1",
    )
    .bind(jti)
    .fetch_optional(&mut *conn)
    .await
}

/// Retire `jti` for a successor, or recognise a straggler, in one
/// `BEGIN IMMEDIATE` transaction so two refreshes of the same token can't
/// both mint. `mint` picks the successor from the predecessor's device kind;
/// it runs only when a successor is created.
pub async fn rotate(
    pool: &SqlitePool,
    jti: Uuid,
    grace_secs: i64,
    mint: impl FnOnce(Option<&str>) -> Successor,
) -> Result<Rotation, sqlx::Error> {
    const MAX_HOPS: usize = 16;
    let now = Utc::now();
    let grace_start = now - Duration::seconds(grace_secs);
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let Some(row) = chain_row(&mut tx, jti).await? else {
        return Ok(Rotation::Rejected);
    };
    let live = row.revoked_at.is_none() && row.session.expires_at > now;
    let outcome = if live
        || (row.rotated_at.is_some_and(|at| at >= grace_start) && row.successor_jti.is_none())
    {
        // A token rotated before families existed has no successor to hand
        // back: its straggler gets one minted, once, like a live token.
        let next = mint(row.session.device_kind.as_deref());
        insert_session(
            &mut *tx,
            &NewSession {
                jti: next.jti,
                family_id: row.family_id,
                user_id: UserId::from(row.session.user_id),
                issued_at: next.issued_at,
                expires_at: next.expires_at,
                device_label: row.session.device_label.as_deref(),
                device_kind: row.session.device_kind.as_deref(),
            },
        )
        .await?;
        sqlx::query(
            "UPDATE refresh_tokens \
             SET revoked_at = COALESCE(revoked_at, ?1), rotated_at = COALESCE(rotated_at, ?1), \
                 successor_jti = ?2 \
             WHERE jti = ?3",
        )
        .bind(now)
        .bind(next.jti)
        .bind(jti)
        .execute(&mut *tx)
        .await?;
        Rotation::Rotated(Session {
            jti: next.jti,
            user_id: row.session.user_id,
            issued_at: next.issued_at,
            expires_at: next.expires_at,
            device_label: row.session.device_label,
            device_kind: row.session.device_kind,
        })
    } else if let Some(rotated_at) = row.rotated_at {
        if rotated_at < grace_start {
            sqlx::query(
                "UPDATE refresh_tokens SET revoked_at = COALESCE(revoked_at, ?1), rotated_at = NULL \
                 WHERE family_id = ?2",
            )
            .bind(now)
            .bind(row.family_id)
            .execute(&mut *tx)
            .await?;
            Rotation::FamilyRevoked
        } else {
            // The winner may have refreshed again since: follow the chain to
            // the family's live head.
            let mut next = row.successor_jti;
            let mut found = Rotation::Rejected;
            for _ in 0..MAX_HOPS {
                let Some(jti) = next else { break };
                let Some(step) = chain_row(&mut tx, jti).await? else {
                    break;
                };
                if step.revoked_at.is_none() && step.session.expires_at > now {
                    found = Rotation::Replayed(step.session);
                    break;
                }
                if step.rotated_at.is_none_or(|at| at < grace_start) {
                    break;
                }
                next = step.successor_jti;
            }
            found
        }
    } else {
        Rotation::Rejected
    };
    tx.commit().await?;
    Ok(outcome)
}

/// The session a claimed pairing code hands its device, replacing (revoking
/// the family of) the one a previous poll handed out, in one transaction: two
/// overlapping polls leave exactly one live session. `false`: the code is gone
/// or unclaimed, nothing was issued.
pub async fn replace_for_device_code(
    pool: &SqlitePool,
    device_id: Uuid,
    new: &NewSession<'_>,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let row: Option<(Option<Uuid>,)> = sqlx::query_as(
        "SELECT session_jti FROM device_codes WHERE device_id = ?1 AND claimed_by IS NOT NULL",
    )
    .bind(device_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((previous,)) = row else {
        return Ok(false);
    };
    if let Some(previous) = previous {
        revoke(&mut *tx, previous).await?;
    }
    insert_session(&mut *tx, new).await?;
    sqlx::query("UPDATE device_codes SET session_jti = ?1 WHERE device_id = ?2")
        .bind(new.jti)
        .bind(device_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

/// Delete sessions nobody can use any more: expired, or revoked/rotated
/// long enough ago that the rotation grace window can't need them.
pub async fn prune(pool: &SqlitePool, before: DateTime<Utc>) -> Result<u64, sqlx::Error> {
    let res = sqlx::query("DELETE FROM refresh_tokens WHERE expires_at < ?1 OR revoked_at < ?1")
        .bind(before)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::migrated_pool;

    fn successor(ttl: Duration) -> Successor {
        let issued_at = Utc::now();
        Successor {
            jti: Uuid::new_v4(),
            issued_at,
            expires_at: issued_at + ttl,
        }
    }

    async fn backdate_rotation(pool: &SqlitePool, jti: Uuid, by: Duration) {
        sqlx::query("UPDATE refresh_tokens SET rotated_at = ?1 WHERE jti = ?2")
            .bind(Utc::now() - by)
            .bind(jti)
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn prune_keeps_live_sessions_only() {
        let pool = migrated_pool().await;
        let user = crate::test_support::make_user(&pool).await;
        let live = Uuid::new_v4();
        let expired = Uuid::new_v4();
        let rotated = Uuid::new_v4();
        insert(&pool, live, user, Utc::now() + Duration::days(7))
            .await
            .unwrap();
        insert(&pool, expired, user, Utc::now() - Duration::days(3))
            .await
            .unwrap();
        insert(&pool, rotated, user, Utc::now() + Duration::days(7))
            .await
            .unwrap();
        let Rotation::Rotated(head) = rotate(&pool, rotated, 60, |_| successor(Duration::days(7)))
            .await
            .unwrap()
        else {
            panic!("a live token rotates");
        };

        // Rotated a moment ago: still inside any grace window, kept.
        assert_eq!(
            prune(&pool, Utc::now() - Duration::days(1)).await.unwrap(),
            1
        );
        // A day on, the rotated one goes too.
        assert_eq!(
            prune(&pool, Utc::now() + Duration::days(1)).await.unwrap(),
            1
        );
        assert!(is_active(&pool, live).await.unwrap());
        assert!(is_active(&pool, head.jti).await.unwrap());
    }

    /// A rotation carries the device tagging and the family forward; the
    /// predecessor is no longer active.
    #[tokio::test]
    async fn a_rotation_hands_the_device_tagging_to_its_successor() {
        let pool = migrated_pool().await;
        let user = crate::test_support::make_user(&pool).await;
        let jti = Uuid::new_v4();
        insert_with_device(
            &pool,
            jti,
            user,
            Utc::now() + Duration::hours(1),
            Some("Living room"),
            Some("android-tv"),
        )
        .await
        .unwrap();
        let mut seen_kind = None;
        let outcome = rotate(&pool, jti, 60, |kind| {
            seen_kind = kind.map(str::to_owned);
            successor(Duration::days(90))
        })
        .await
        .unwrap();
        assert_eq!(seen_kind.as_deref(), Some("android-tv"));
        let Rotation::Rotated(next) = outcome else {
            panic!("{outcome:?}");
        };
        assert_eq!(next.device_label.as_deref(), Some("Living room"));
        assert!(!is_active(&pool, jti).await.unwrap());
        assert!(is_active(&pool, next.jti).await.unwrap());
        let listed = list_devices_for_user(&pool, user).await.unwrap();
        assert_eq!(listed.iter().map(|t| t.jti).collect::<Vec<_>>(), [next.jti]);
    }

    /// The grace contract: a straggler within the window gets the SAME
    /// successor, never a session of its own; after the window the replay
    /// ends the family.
    #[tokio::test]
    async fn a_replay_within_grace_gets_the_same_successor_and_after_it_revokes_the_family() {
        let pool = migrated_pool().await;
        let user = crate::test_support::make_user(&pool).await;
        let first = Uuid::new_v4();
        insert(&pool, first, user, Utc::now() + Duration::days(7))
            .await
            .unwrap();
        let Rotation::Rotated(second) = rotate(&pool, first, 60, |_| successor(Duration::days(7)))
            .await
            .unwrap()
        else {
            panic!("a live token rotates");
        };

        let replay = rotate(&pool, first, 60, |_| panic!("a straggler never mints"))
            .await
            .unwrap();
        assert_eq!(replay, Rotation::Replayed(second.clone()));

        // The winner refreshed again meanwhile: the straggler follows the
        // chain to the live head.
        let Rotation::Rotated(third) =
            rotate(&pool, second.jti, 60, |_| successor(Duration::days(7)))
                .await
                .unwrap()
        else {
            panic!("the head rotates");
        };
        assert_eq!(
            rotate(&pool, first, 60, |_| panic!("no mint"))
                .await
                .unwrap(),
            Rotation::Replayed(third.clone())
        );
        let tokens: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM refresh_tokens")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(tokens, 3, "replays minted nothing");

        backdate_rotation(&pool, first, Duration::minutes(5)).await;
        assert_eq!(
            rotate(&pool, first, 60, |_| panic!("no mint"))
                .await
                .unwrap(),
            Rotation::FamilyRevoked
        );
        assert!(!is_active(&pool, third.jti).await.unwrap());
        assert_eq!(
            rotate(&pool, third.jti, 60, |_| panic!("no mint"))
                .await
                .unwrap(),
            Rotation::Rejected
        );
    }

    /// Logout and device revoke end the family: neither the live head nor a
    /// just-rotated token comes back.
    #[tokio::test]
    async fn logout_and_device_revoke_end_the_whole_family() {
        let pool = migrated_pool().await;
        let user = crate::test_support::make_user(&pool).await;
        for by_device in [false, true] {
            let first = Uuid::new_v4();
            insert_with_device(
                &pool,
                first,
                user,
                Utc::now() + Duration::days(7),
                None,
                Some("android-tv"),
            )
            .await
            .unwrap();
            let Rotation::Rotated(head) =
                rotate(&pool, first, 60, |_| successor(Duration::days(7)))
                    .await
                    .unwrap()
            else {
                panic!("rotates");
            };
            if by_device {
                let other = crate::test_support::make_user(&pool).await;
                assert!(!revoke_for_user(&pool, other, head.jti).await.unwrap());
                assert!(revoke_for_user(&pool, user, head.jti).await.unwrap());
            } else {
                revoke(&pool, head.jti).await.unwrap();
            }
            assert!(!is_active(&pool, head.jti).await.unwrap());
            assert_eq!(
                rotate(&pool, first, 60, |_| panic!("no mint"))
                    .await
                    .unwrap(),
                Rotation::Rejected,
                "by_device={by_device}"
            );
        }
    }

    #[tokio::test]
    async fn a_token_rotated_before_families_mints_its_successor_once() {
        let pool = migrated_pool().await;
        let user = crate::test_support::make_user(&pool).await;
        let old = Uuid::new_v4();
        insert(&pool, old, user, Utc::now() + Duration::days(7))
            .await
            .unwrap();
        sqlx::query(
            "UPDATE refresh_tokens SET revoked_at = ?1, rotated_at = ?1, family_id = NULL \
             WHERE jti = ?2",
        )
        .bind(Utc::now())
        .bind(old)
        .execute(&pool)
        .await
        .unwrap();
        let Rotation::Rotated(minted) = rotate(&pool, old, 60, |_| successor(Duration::days(7)))
            .await
            .unwrap()
        else {
            panic!("a pre-family straggler gets a successor");
        };
        assert_eq!(
            rotate(&pool, old, 60, |_| panic!("once")).await.unwrap(),
            Rotation::Replayed(minted)
        );
    }

    #[tokio::test]
    async fn two_overlapping_polls_leave_one_live_device_session() {
        let pool = migrated_pool().await;
        let user = crate::test_support::make_user(&pool).await;
        let code = crate::device_codes::create(
            &pool,
            "ABCD-2345",
            Utc::now() + Duration::minutes(10),
            "android-tv",
        )
        .await
        .unwrap();
        let session = |jti| NewSession {
            jti,
            family_id: jti,
            user_id: user,
            issued_at: Utc::now(),
            expires_at: Utc::now() + Duration::days(90),
            device_label: None,
            device_kind: Some("android-tv"),
        };
        let early = Uuid::new_v4();
        assert!(
            !replace_for_device_code(&pool, code.device_id, &session(early))
                .await
                .unwrap(),
            "an unclaimed code issues nothing"
        );
        assert!(
            crate::device_codes::claim(&pool, "ABCD-2345", user, None)
                .await
                .unwrap()
        );
        let (a, b) = (session(Uuid::new_v4()), session(Uuid::new_v4()));
        let (ra, rb) = tokio::join!(
            replace_for_device_code(&pool, code.device_id, &a),
            replace_for_device_code(&pool, code.device_id, &b),
        );
        assert!(ra.unwrap() && rb.unwrap());
        let live = list_devices_for_user(&pool, user).await.unwrap();
        assert_eq!(live.len(), 1);
        let row = crate::device_codes::find_by_device_id(&pool, code.device_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.session_jti, Some(live[0].jti));
    }

    #[tokio::test]
    async fn an_expired_device_session_is_not_listed() {
        let pool = migrated_pool().await;
        let user = crate::test_support::make_user(&pool).await;
        let live = Uuid::new_v4();
        for (jti, expires_at) in [
            (live, Utc::now() + Duration::days(1)),
            (Uuid::new_v4(), Utc::now() - Duration::minutes(1)),
        ] {
            insert_with_device(&pool, jti, user, expires_at, None, Some("android-tv"))
                .await
                .unwrap();
        }
        let listed = list_devices_for_user(&pool, user).await.unwrap();
        assert_eq!(listed.iter().map(|t| t.jti).collect::<Vec<_>>(), [live]);
    }
}
