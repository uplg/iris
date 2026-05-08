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
