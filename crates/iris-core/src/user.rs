use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::UserId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub email: String,
    /// Public-facing handle, exposed everywhere `added_by_name` /
    /// "shared with X" appears. Defaults to the email's local-part on
    /// account creation; the user can edit it from /account.
    pub display_name: String,
    pub is_admin: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    User,
}

impl Role {
    pub fn from_is_admin(is_admin: bool) -> Self {
        if is_admin { Self::Admin } else { Self::User }
    }
}
