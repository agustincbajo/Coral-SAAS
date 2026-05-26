//! Single error type returned by handlers.
//!
//! Maps to an HTTP response via `IntoResponse`. The `From<sqlx::Error>` /
//! `From<reqwest::Error>` impls let route handlers use `?` freely while
//! still surfacing a sane status code + JSON body to the client.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
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

    #[error("rate limited")]
    RateLimited,

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("redis error: {0}")]
    Redis(#[from] redis::RedisError),

    #[error("http client error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("jwt error: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("internal: {0}")]
    Internal(#[from] anyhow::Error),
}

impl ApiError {
    fn status(&self) -> StatusCode {
        match self {
            ApiError::NotFound => StatusCode::NOT_FOUND,
            ApiError::Unauthorized => StatusCode::UNAUTHORIZED,
            ApiError::Forbidden => StatusCode::FORBIDDEN,
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Conflict(_) => StatusCode::CONFLICT,
            ApiError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            ApiError::Database(sqlx::Error::RowNotFound) => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn user_message(&self) -> String {
        match self {
            ApiError::NotFound => "not found".to_string(),
            ApiError::Unauthorized => "unauthorized".to_string(),
            ApiError::Forbidden => "forbidden".to_string(),
            ApiError::BadRequest(m) | ApiError::Conflict(m) => m.clone(),
            ApiError::RateLimited => "rate limited".to_string(),
            ApiError::Database(sqlx::Error::RowNotFound) => "not found".to_string(),
            // 5xx errors are scrubbed to avoid leaking internals; the
            // full error is logged with the request_id for ops to find.
            _ => "internal server error".to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let user_msg = self.user_message();

        if status.is_server_error() {
            tracing::error!(error = ?self, "request failed with 5xx");
        } else {
            tracing::warn!(error = %self, status = status.as_u16(), "request failed with 4xx");
        }

        (
            status,
            Json(json!({
                "error": user_msg,
                "status": status.as_u16(),
            })),
        )
            .into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
