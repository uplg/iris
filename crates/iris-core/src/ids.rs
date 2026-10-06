use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl From<Uuid> for $name {
            fn from(u: Uuid) -> Self {
                Self(u)
            }
        }

        impl From<$name> for Uuid {
            fn from(v: $name) -> Self {
                v.0
            }
        }
    };
}

id_type!(UserId);
id_type!(InvitationId);
id_type!(TorrentId);
id_type!(SessionId);

/// A v1 `BitTorrent` infohash in hex: 40 hex digits, either case.
#[must_use]
pub fn is_infohash_hex(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::is_infohash_hex;

    #[test]
    fn validates_infohash() {
        assert!(is_infohash_hex("98259ba623eec5f33167c083b51b30122c7fa068"));
        assert!(is_infohash_hex("ABCDEF0123456789abcdef0123456789ABCDEF01"));
        // Wrong length.
        assert!(!is_infohash_hex("98259ba6"));
        assert!(!is_infohash_hex(""));
        // Non-hex char.
        assert!(!is_infohash_hex("98259ba623eec5f33167c083b51b30122c7fa06z"));
        // Numeric guid (e.g. UNIT3D torrent id) — not an infohash.
        assert!(!is_infohash_hex("12345"));
    }
}
