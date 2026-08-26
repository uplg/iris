use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("forbidden")]
    Forbidden,

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("upstream provider error: {0}")]
    Provider(String),

    /// The tracker answered the request but refused to serve it — the
    /// account is resolved, a policy said no. Distinct from [`Self::Provider`]
    /// so the API can hand the user an actionable message instead of a raw
    /// `401 Unauthorized` (UNIT3D 302s a refused `.torrent` download to the
    /// torrent page, which our JSON `Accept` turns into a Laravel 401).
    #[error("provider refused the request: {0}")]
    ProviderRefused(String),

    #[error("internal: {0}")]
    Internal(String),
}

impl Error {
    pub fn internal<E: std::fmt::Display>(e: E) -> Self {
        Self::Internal(e.to_string())
    }

    pub fn invalid<S: Into<String>>(msg: S) -> Self {
        Self::InvalidInput(msg.into())
    }
}
