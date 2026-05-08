//! Authentication primitives: password hashing, JWT issuance/verification,
//! and invitation token generation.
//!
//! All persistence lives in `iris-db`; this crate is pure functions over
//! cryptographic primitives so it stays easy to test.

pub mod invitation;
pub mod jwt;
pub mod password;

pub use invitation::{InvitationToken, hash_invitation_token, new_invitation_token};
pub use jwt::{AccessClaims, Issuer, RefreshClaims, TokenKind};
pub use password::{hash_password, verify_password};
