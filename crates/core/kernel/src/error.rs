// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use contracts::ContractIssue;
use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("validation failed: {message}")]
    Validation {
        message: String,
        issues: Vec<ContractIssue>,
    },

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("internal: {0}")]
    Internal(String),

    #[error("cancelled: {0}")]
    Cancelled(String),

    #[error("resource exhausted: {0}")]
    ResourceExhausted(String),

    #[error("payload too large: {0}")]
    PayloadTooLarge(String),

    #[error("unavailable: {0}")]
    Unavailable(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl Error {
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::InvalidArgument(msg.into())
    }

    pub fn validation(message: impl Into<String>, issues: Vec<ContractIssue>) -> Self {
        Self::Validation {
            message: message.into(),
            issues,
        }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::Conflict(msg.into())
    }

    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::Forbidden(msg.into())
    }

    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::Unauthorized(msg.into())
    }

    pub fn cancelled(msg: impl Into<String>) -> Self {
        Self::Cancelled(msg.into())
    }

    /// Resource admission exhausted (429 Too Many Requests): rejects when concurrency slots are
    /// full or the work group quota is exhausted.
    pub fn resource_exhausted(msg: impl Into<String>) -> Self {
        Self::ResourceExhausted(msg.into())
    }

    /// Request body too large / storage quota exceeded (413 Payload Too Large): returned when
    /// intake of profiles and the like exceeds `max_storage_bytes`; the caller writes no object.
    pub fn payload_too_large(msg: impl Into<String>) -> Self {
        Self::PayloadTooLarge(msg.into())
    }

    /// Service unavailable (503 Service Unavailable): rejects while the node is draining and
    /// not taking new work.
    pub fn unavailable(msg: impl Into<String>) -> Self {
        Self::Unavailable(msg.into())
    }

    /// Semantic mapping to an HTTP status code. Any HTTP adapter layer can use this value.
    pub fn http_status_code(&self) -> u16 {
        match self {
            Error::NotFound(_) => 404,
            Error::Conflict(_) => 409,
            Error::InvalidArgument(_) | Error::Validation { .. } => 400,
            Error::Unauthorized(_) => 401,
            Error::Forbidden(_) => 403,
            Error::ResourceExhausted(_) => 429,
            Error::PayloadTooLarge(_) => 413,
            // 499 is nginx-style client cancellation; standard 4xx has no dedicated status code,
            // so the closest semantics are reused.
            Error::Cancelled(_) => 499,
            Error::Unavailable(_) => 503,
            Error::Internal(_) | Error::Other(_) => 500,
        }
    }

    /// Unified error code string for the frontend and API clients.
    pub fn http_error_code(&self) -> &'static str {
        match self {
            Error::NotFound(_) => "not_found",
            Error::Conflict(_) => "conflict",
            Error::InvalidArgument(_) => "invalid_argument",
            Error::Validation { .. } => "validation_failed",
            Error::Unauthorized(_) => "unauthorized",
            Error::Forbidden(_) => "forbidden",
            Error::ResourceExhausted(_) => "resource_exhausted",
            Error::PayloadTooLarge(_) => "payload_too_large",
            Error::Cancelled(_) => "cancelled",
            Error::Unavailable(_) => "unavailable",
            Error::Internal(_) | Error::Other(_) => "internal",
        }
    }
}

#[cfg(feature = "http")]
mod axum_impl {
    use axum::{
        Json,
        http::StatusCode,
        response::{IntoResponse, Response},
    };
    use serde_json::json;

    use super::Error;

    impl IntoResponse for Error {
        fn into_response(self) -> Response {
            let status = StatusCode::from_u16(self.http_status_code())
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

            // 5xx: log the details to tracing (clients cannot see them, which avoids leaking
            // internal details, but ops can locate the cause).
            if status.is_server_error() {
                tracing::error!(error = ?self, "request failed with server error");
            }

            // Client response: 5xx strips the message; 4xx sends the message as is, which is
            // easier for clients to understand.
            let message = match &self {
                Error::Internal(_) | Error::Other(_) => "internal error".to_string(),
                other => other.to_string(),
            };

            let body = match &self {
                Error::Validation { issues, .. } => Json(json!({
                    "error": self.http_error_code(),
                    "message": message,
                    "issues": issues,
                })),
                _ => Json(json!({
                    "error": self.http_error_code(),
                    "message": message,
                })),
            };

            (status, body).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_code_mapping() {
        assert_eq!(Error::NotFound("x".into()).http_status_code(), 404);
        assert_eq!(Error::Conflict("x".into()).http_status_code(), 409);
        assert_eq!(Error::InvalidArgument("x".into()).http_status_code(), 400);
        assert_eq!(Error::validation("x", Vec::new()).http_status_code(), 400);
        assert_eq!(Error::Unauthorized("x".into()).http_status_code(), 401);
        assert_eq!(Error::Forbidden("x".into()).http_status_code(), 403);
        assert_eq!(Error::Unavailable("x".into()).http_status_code(), 503);
        assert_eq!(Error::ResourceExhausted("x".into()).http_status_code(), 429);
        assert_eq!(Error::PayloadTooLarge("x".into()).http_status_code(), 413);
        assert_eq!(Error::Internal("x".into()).http_status_code(), 500);
    }

    #[test]
    fn error_code_strings_stable() {
        assert_eq!(Error::NotFound("x".into()).http_error_code(), "not_found");
        assert_eq!(
            Error::InvalidArgument("x".into()).http_error_code(),
            "invalid_argument"
        );
        assert_eq!(
            Error::validation("x", Vec::new()).http_error_code(),
            "validation_failed"
        );
    }
}
