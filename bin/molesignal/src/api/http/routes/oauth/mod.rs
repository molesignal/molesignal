// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! OAuth 2.1 authorization server for the Inbound MCP protected resource.

use axum::{
    Json, Router,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;

use crate::{api::AppState, shared::Error};

mod authorization;
mod client;
pub(crate) mod metadata;
mod registration;
mod token;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(authorization::routes())
        .merge(registration::routes())
        .merge(token::routes())
}

pub fn well_known_routes() -> Router<AppState> {
    metadata::routes()
}

#[derive(Debug, Serialize)]
struct OAuthErrorBody {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_description: Option<String>,
}

#[derive(Debug)]
struct OAuthError {
    status: StatusCode,
    code: &'static str,
    description: String,
}

impl OAuthError {
    fn invalid_request(description: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_request",
            description: description.into(),
        }
    }

    fn invalid_client(description: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "invalid_client",
            description: description.into(),
        }
    }

    fn invalid_grant(description: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_grant",
            description: description.into(),
        }
    }

    fn invalid_scope(description: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_scope",
            description: description.into(),
        }
    }

    fn unsupported_grant_type() -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "unsupported_grant_type",
            description: "supported grant types are authorization_code and refresh_token".into(),
        }
    }

    fn server(error: Error) -> Self {
        tracing::warn!(error = %error, "OAuth request failed");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "server_error",
            description: "the authorization server could not complete the request".into(),
        }
    }
}

impl IntoResponse for OAuthError {
    fn into_response(self) -> Response {
        let authenticate = self.code == "invalid_client";
        let mut response = (
            self.status,
            [
                (header::CACHE_CONTROL, "no-store"),
                (header::PRAGMA, "no-cache"),
            ],
            Json(OAuthErrorBody {
                error: self.code.into(),
                error_description: Some(self.description),
            }),
        )
            .into_response();
        if authenticate {
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                http::HeaderValue::from_static("Basic realm=\"MoleSignal OAuth token endpoint\""),
            );
        }
        response
    }
}

type OAuthResult<T> = std::result::Result<T, OAuthError>;
