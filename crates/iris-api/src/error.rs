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
