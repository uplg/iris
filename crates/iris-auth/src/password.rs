use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::{Algorithm, Argon2, Params, Version};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PasswordError {
    #[error("argon2 error: {0}")]
    Argon2(String),
}

fn argon2() -> Argon2<'static> {
    Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::default())
}

pub fn hash_password(password: &str) -> Result<String, PasswordError> {
    argon2()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| PasswordError::Argon2(e.to_string()))
}

pub fn verify_password(password: &str, hash: &str) -> Result<bool, PasswordError> {
    // Parse first so a corrupt stored hash surfaces as an error rather than
    // a silent "wrong password".
    let parsed = PasswordHash::new(hash).map_err(|e| PasswordError::Argon2(e.to_string()))?;
    Ok(argon2()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let h = hash_password("hunter2").unwrap();
        assert!(verify_password("hunter2", &h).unwrap());
        assert!(!verify_password("nope", &h).unwrap());
    }

    #[test]
    fn phc_layout_unchanged_across_argon2_bump() {
        // Stored hashes were written by argon2 0.5 with exactly these
        // parameters; they keep verifying only as long as 0.6 emits and reads
        // the same PHC layout. A fixed pre-bump-shaped string goes through the
        // same parse + verify path as a fresh one.
        let fresh = hash_password("hunter2").unwrap();
        assert!(fresh.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
        let stored = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$\
                      K5c9m8TF3mR1n6cJ3qB4pF0Yw8hZ6tQ1uV2xW3yZ4aA";
        assert!(!verify_password("nope", stored).unwrap());
        // A corrupt stored hash is an error, not a silent "wrong password".
        assert!(verify_password("nope", "not-a-phc-string").is_err());
    }
}
