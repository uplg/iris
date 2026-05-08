use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngExt;
use sha2::{Digest, Sha256};

const TOKEN_BYTES: usize = 32;

#[derive(Debug, Clone)]
pub struct InvitationToken {
    /// Plaintext token, shown to the inviter ONCE.
    pub plaintext: String,
    /// Hashed form persisted in the DB (lookup uses this).
    pub hash: String,
}

pub fn new_invitation_token() -> InvitationToken {
    let mut buf = [0u8; TOKEN_BYTES];
    rand::rng().fill(&mut buf[..]);
    let plaintext = URL_SAFE_NO_PAD.encode(buf);
    let hash = hash_invitation_token(&plaintext);
    InvitationToken { plaintext, hash }
}

pub fn hash_invitation_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_hash() {
        let a = hash_invitation_token("foo");
        let b = hash_invitation_token("foo");
        assert_eq!(a, b);
    }

    #[test]
    fn unique_tokens() {
        let a = new_invitation_token();
        let b = new_invitation_token();
        assert_ne!(a.plaintext, b.plaintext);
    }
}
