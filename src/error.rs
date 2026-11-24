//! Unified application error type and Axum JSON error response mapping.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use thiserror::Error;

/// JSON error payload returned by ToolStow HTTP endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErrorResponse {
    /// Machine-readable error code (e.g. `"BAD_REQUEST"`).
    pub code: &'static str,
    /// Human-readable error message.
    pub error: String,
}

/// Unified domain and HTTP error type across ToolStow modules.
#[derive(Debug, Error)]
pub enum AppError {
    /// Client sent a malformed or invalid request (`400 Bad Request`).
    #[error("{0}")]
    BadRequest(String),

    /// Client failed authentication or supplied invalid credentials (`401 Unauthorized`).
    #[error("{0}")]
    Unauthorized(String),

    /// Request blocked by security policy, such as CSRF/Origin checks (`403 Forbidden`).
    #[error("{0}")]
    Forbidden(String),

    /// Requested tool or asset does not exist (`404 Not Found`).
    #[error("{0}")]
    NotFound(String),

    /// Upload archive or decompressed payload exceeds configured limits (`413 Payload Too Large`).
    #[error("{0}")]
    PayloadTooLarge(String),

    /// Manifest or archive contents failed semantic validation (`422 Unprocessable Entity`).
    #[error("{0}")]
    UnprocessableEntity(String),

    /// Client exceeded authentication or request rate limits (`429 Too Many Requests`).
    #[error("{0}")]
    TooManyRequests(String),

    /// Unexpected server-side error (`500 Internal Server Error`).
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    /// Constructs a `400 Bad Request` error.
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    /// Constructs a `401 Unauthorized` error.
    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::Unauthorized(msg.into())
    }

    /// Constructs a `403 Forbidden` error.
    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::Forbidden(msg.into())
    }

    /// Constructs a `404 Not Found` error.
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    /// Constructs a `413 Payload Too Large` error.
    pub fn payload_too_large(msg: impl Into<String>) -> Self {
        Self::PayloadTooLarge(msg.into())
    }

    /// Constructs a `422 Unprocessable Entity` error.
    pub fn unprocessable(msg: impl Into<String>) -> Self {
        Self::UnprocessableEntity(msg.into())
    }

    /// Constructs a `429 Too Many Requests` error.
    pub fn too_many_requests(msg: impl Into<String>) -> Self {
        Self::TooManyRequests(msg.into())
    }

    /// Constructs a `500 Internal Server Error`.
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    /// Maps this error variant to its HTTP status code.
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::PayloadTooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            Self::UnprocessableEntity(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::TooManyRequests(_) => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Maps this error variant to its stable machine-readable code string.
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "BAD_REQUEST",
            Self::Unauthorized(_) => "unauthorized",
            Self::Forbidden(_) => "FORBIDDEN",
            Self::NotFound(_) => "NOT_FOUND",
            Self::PayloadTooLarge(_) => "PAYLOAD_TOO_LARGE",
            Self::UnprocessableEntity(_) => "UNPROCESSABLE_ENTITY",
            Self::TooManyRequests(_) => "too_many_requests",
            Self::Internal(_) => "INTERNAL_ERROR",
        }
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        Self::Internal(err.to_string())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let code = self.error_code();
        let error = self.to_string();

        if matches!(self, Self::Internal(_)) {
            tracing::error!(code, error = %error, "internal server error");
        }

        let body = ErrorResponse { code, error };
        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    async fn response_parts(err: AppError) -> (StatusCode, serde_json::Value) {
        let resp = err.into_response();
        let status = resp.status();
        let content_type = resp
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            content_type.starts_with("application/json"),
            "expected application/json content-type, got {content_type:?}"
        );
        let bytes = resp
            .into_body()
            .collect()
            .await
            .expect("body should collect")
            .to_bytes();
        let json: serde_json::Value =
            serde_json::from_slice(&bytes).expect("body should be valid json");
        (status, json)
    }

    #[tokio::test]
    async fn maps_all_variants_to_expected_status_and_json() {
        let cases = vec![
            (
                AppError::bad_request("missing zip field"),
                StatusCode::BAD_REQUEST,
                "BAD_REQUEST",
                "missing zip field",
            ),
            (
                AppError::unauthorized("invalid admin key"),
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "invalid admin key",
            ),
            (
                AppError::forbidden("cross-site origin rejected"),
                StatusCode::FORBIDDEN,
                "FORBIDDEN",
                "cross-site origin rejected",
            ),
            (
                AppError::not_found("tool not found"),
                StatusCode::NOT_FOUND,
                "NOT_FOUND",
                "tool not found",
            ),
            (
                AppError::payload_too_large("archive exceeds 50 MB"),
                StatusCode::PAYLOAD_TOO_LARGE,
                "PAYLOAD_TOO_LARGE",
                "archive exceeds 50 MB",
            ),
            (
                AppError::unprocessable("invalid manifest.json"),
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                "invalid manifest.json",
            ),
            (
                AppError::too_many_requests("rate limit exceeded"),
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_requests",
                "rate limit exceeded",
            ),
            (
                AppError::internal("disk I/O failure"),
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "disk I/O failure",
            ),
        ];

        for (err, expected_status, expected_code, expected_msg) in cases {
            let (status, json) = response_parts(err).await;
            assert_eq!(status, expected_status);
            assert_eq!(
                json,
                serde_json::json!({
                    "code": expected_code,
                    "error": expected_msg,
                })
            );
        }
    }

    #[test]
    fn converts_from_anyhow_as_internal_error() {
        let err: AppError = anyhow::anyhow!("boom").into();
        assert_eq!(err.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(err.error_code(), "INTERNAL_ERROR");
        assert_eq!(err.to_string(), "boom");
    }
}
