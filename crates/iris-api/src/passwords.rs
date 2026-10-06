//! The one place passwords are checked, hashed and verified.
//!
//! Argon2 costs about 100 ms of CPU per call, so hashing and verifying run on
//! Tokio's blocking pool: on an async worker they would stall every other
//! request scheduled there. At most one Argon2 run per core at a time: each
//! holds ~19 MiB, and under a login flood the blocking pool would otherwise
//! grow to 512 of them.

use std::sync::LazyLock;

use tokio::sync::{OnceCell, Semaphore};

use crate::error::{ApiError, ApiResult};

pub const MIN_LEN: usize = 8;

static ARGON2_SLOTS: LazyLock<Semaphore> = LazyLock::new(|| {
    Semaphore::new(std::thread::available_parallelism().map_or(2, std::num::NonZero::get))
});

async fn run_argon2<T: Send + 'static>(
    what: &'static str,
    job: impl FnOnce() -> T + Send + 'static,
) -> ApiResult<T> {
    let _slot = ARGON2_SLOTS
        .acquire()
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("{what} slot: {e}")))?;
    tokio::task::spawn_blocking(job)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("{what} task: {e}")))
}

/// Reject a password the household policy doesn't accept.
pub fn check_policy(password: &str) -> ApiResult<()> {
    if password.len() < MIN_LEN {
        return Err(ApiError::BadRequest(format!(
            "password too short (min {MIN_LEN} chars)"
        )));
    }
    Ok(())
}

pub async fn hash(password: &str) -> ApiResult<String> {
    let password = password.to_owned();
    run_argon2("hash", move || iris_auth::hash_password(&password))
        .await?
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("hash: {e}")))
}

pub async fn verify(password: &str, stored_hash: &str) -> ApiResult<bool> {
    let password = password.to_owned();
    let stored_hash = stored_hash.to_owned();
    run_argon2("verify", move || {
        iris_auth::verify_password(&password, &stored_hash)
    })
    .await?
    .map_err(|e| ApiError::Internal(anyhow::anyhow!("verify: {e}")))
}

/// Spend a real verify's time on an unknown account, so a login miss takes
/// as long as a wrong password and response time can't enumerate emails.
pub async fn verify_nobody(password: &str) {
    static DUMMY: OnceCell<String> = OnceCell::const_new();
    let Ok(dummy) = DUMMY
        .get_or_try_init(|| hash("never-matches-by-design"))
        .await
    else {
        return;
    };
    let _ = verify(password, dummy).await;
}

#[cfg(test)]
mod tests {
    use super::{MIN_LEN, check_policy, hash, verify, verify_nobody};

    #[test]
    fn policy_rejects_short_passwords() {
        assert!(check_policy(&"x".repeat(MIN_LEN - 1)).is_err());
        assert!(check_policy(&"x".repeat(MIN_LEN)).is_ok());
    }

    #[tokio::test]
    async fn hash_then_verify_on_the_blocking_pool() {
        let h = hash("hunter22").await.unwrap();
        assert!(verify("hunter22", &h).await.unwrap());
        assert!(!verify("hunter23", &h).await.unwrap());
    }

    #[tokio::test]
    async fn verify_nobody_hashes_its_dummy_once() {
        verify_nobody("whatever").await;
        verify_nobody("whatever").await;
    }
}
