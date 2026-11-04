// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 登录限速在 HTTP 这边的部分：这个请求的来源，和拒绝时的回答。规则在
//! [`crate::app::iam::signin_limit`]。

use std::{
    convert::Infallible,
    net::{IpAddr, SocketAddr},
};

use axum::{
    Json,
    extract::{ConnectInfo, FromRequestParts},
    http::{HeaderValue, StatusCode, header::RETRY_AFTER, request::Parts},
    response::{IntoResponse, Response},
};
use serde_json::json;

use super::client_ip::ClientIpResolverHandle;
use crate::app::iam::signin_limit::{SigninError, SigninRefused};

/// 这个请求的来源，见 [`ClientIpResolverHandle::signin_source`]。
pub(crate) struct SigninSource(pub Option<IpAddr>);

impl<S: Send + Sync> FromRequestParts<S> for SigninSource {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(address)| *address);

        let source = parts
            .extensions
            .get::<ClientIpResolverHandle>()
            .and_then(|resolver| resolver.signin_source(&parts.headers, peer));

        Ok(Self(source))
    }
}

impl IntoResponse for SigninRefused {
    fn into_response(self) -> Response {
        let body = json!({
            "error": "resource_exhausted",
            "code": "signin_limited",
            "message": self.message(),
        });

        let mut response = (StatusCode::TOO_MANY_REQUESTS, Json(body)).into_response();

        response
            .headers_mut()
            .insert(RETRY_AFTER, HeaderValue::from(self.retry_after_secs));

        response
    }
}

impl IntoResponse for SigninError {
    fn into_response(self) -> Response {
        match self {
            Self::Refused(refused) => refused.into_response(),
            Self::Failed(error) => error.into_response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;

    use super::*;

    #[tokio::test]
    async fn a_refusal_says_when_to_try_again() {
        let response = SigninRefused {
            retry_after_secs: 840,
        }
        .into_response();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()[RETRY_AFTER], "840");

        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body["code"], "signin_limited");
        assert_eq!(
            body["message"],
            "too many failed password attempts; try again in 14 minutes"
        );
    }
}
