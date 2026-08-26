use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("not found")]
    NotFound,
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("conflict: {0}")]
    Conflict(String),
    /// The chosen release has 0 seeders — grabbing it would never complete.
    /// Distinct code so clients can show a "dead torrent" message.
    #[error("this release has no seeders and can't be downloaded")]
    DeadTorrent,
    /// The release is a packed archive set (scene RAR volumes) with no
    /// playable video — Iris streams straight from the container, so it
    /// could be seeded but never watched. Distinct code so clients can
    /// steer the user to an unrar'd release.
    #[error(
        "this release is packed in RAR archives and can't be streamed — pick an unrar'd release"
    )]
    ArchiveOnly,
    /// The same movie is already in the library under another release.
    /// Distinct code so clients can offer an explicit "download another
    /// copy" confirmation (retry with `allow_duplicate`). The message
    /// carries the existing copies for display.
    #[error("{0}")]
    DuplicateInLibrary(String),
    /// The tracker answered but declined to serve the release — a
    /// download-slot limit, revoked download rights, a moderation state.
    /// Distinct code so clients show the provider's actual reason instead of
    /// the raw `401 Unauthorized` the refusal arrives as.
    #[error("{0}")]
    ProviderRefused(String),
    /// The provider caps concurrent downloads and the cap is reached. Held
    /// apart from [`Self::ProviderRefused`] because we detect it *before*
    /// asking the tracker, and the message names what's holding the slot.
    #[error("{0}")]
    ProviderSlotLimit(String),
    /// A remote origin we depend on (live TV playlist / stream / guide)
    /// failed. 502 so clients can distinguish "their side" from "our side".
    #[error("upstream unavailable: {0}")]
    Upstream(String),
    #[error("internal: {0}")]
    Internal(#[from] anyhow::Error),
    #[error("database: {0}")]
    Db(#[from] sqlx::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, msg) = match &self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not_found", self.to_string()),
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized", self.to_string()),
            ApiError::Forbidden => (StatusCode::FORBIDDEN, "forbidden", self.to_string()),
            ApiError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request", self.to_string()),
            ApiError::Conflict(_) => (StatusCode::CONFLICT, "conflict", self.to_string()),
            ApiError::DeadTorrent => (StatusCode::CONFLICT, "dead_torrent", self.to_string()),
            ApiError::ArchiveOnly => (StatusCode::CONFLICT, "archive_only", self.to_string()),
            ApiError::DuplicateInLibrary(_) => (
                StatusCode::CONFLICT,
                "duplicate_in_library",
                self.to_string(),
            ),
            ApiError::ProviderRefused(_) => {
                (StatusCode::CONFLICT, "provider_refused", self.to_string())
            }
            ApiError::ProviderSlotLimit(_) => (
                StatusCode::CONFLICT,
                "provider_slot_limit",
                self.to_string(),
            ),
            ApiError::Upstream(_) => (StatusCode::BAD_GATEWAY, "upstream", self.to_string()),
            ApiError::Db(_) | ApiError::Internal(_) => {
                tracing::error!(error = ?self, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "internal server error".into(),
                )
            }
        };
        (status, Json(json!({ "error": code, "message": msg }))).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
