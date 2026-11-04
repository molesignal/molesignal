// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 外部告警的接入地址：`POST /api/v1/intake/alerts`。
//!
//! 外部系统带来源的 Token 调用：`Authorization: Bearer mspat-<36>`；设不了请求头的（阿里云云
//! 监控、腾讯云可观测平台、经 SNS 的 CloudWatch）把 Token 放在地址上 `?token=`。鉴权层放行这个路径，
//! 由这里按 Token 认出来源，Token 不对一律回 401。请求体按来源的类型读，最大 4 MiB。所有来源共用这一个
//! 地址；联邦的每个集群都有这个地址，发到哪个集群都行。
//!
//! CloudWatch 的告警经 SNS 过来：收请求的节点先验 SNS 的签名，订阅确认也在这里做；转给接收集群的是验过的。

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, RawQuery, State},
    http::{
        HeaderMap, StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE},
    },
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::Value;

use crate::{
    api::{AppState, http::routes::notify::callback_auth::sns_checked},
    app::alerting::sources::IntakeReply,
    domain::alerting::source::AlertSourceKind,
    shared::Result,
};

/// 接入地址的路径。
pub const INTAKE_PATH: &str = "/api/v1/intake/alerts";
/// 请求体最大多少字节。
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

pub fn public_routes() -> Router<AppState> {
    Router::new().route(
        INTAKE_PATH,
        post(intake).layer(DefaultBodyLimit::max(MAX_BODY_BYTES)),
    )
}

async fn intake(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let presented = bearer(&headers).or_else(|| query_token(query.as_deref()));
    let sources = &state.alerting.sources;

    let source = match sources.authenticate(presented.as_deref()).await {
        Ok(source) => source,
        Err(error) => return error.into_response(),
    };

    if let Err(error) = state
        .iam
        .service
        .ensure_organization_access(&source.org_id)
        .await
    {
        return error.into_response();
    }

    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());

    if source.enabled
        && source.kind == AlertSourceKind::AwsCloudwatch
        && let Err(error) = check_sns(&body).await
    {
        return answer(sources.refuse(&source, &error, &body).await);
    }

    answer(sources.intake(&source, content_type, &body).await)
}

fn answer(reply: IntakeReply) -> Response {
    let status = StatusCode::from_u16(reply.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

    (status, Json(reply.body)).into_response()
}

/// SNS 发来的要有对的签名。没有信封的（Raw message delivery 开着）留给读内容的那一步去说。
async fn check_sns(body: &[u8]) -> Result<()> {
    let Ok(notice) = serde_json::from_slice::<Value>(body) else {
        return Ok(());
    };

    if notice.get("Type").is_none() {
        return Ok(());
    }

    sns_checked(&notice).await.map(|_| ())
}

/// `Authorization: Bearer <token>`。
fn bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
}

/// 地址上的 `?token=`，按表单编码解开。
fn query_token(query: Option<&str>) -> Option<String> {
    url::form_urlencoded::parse(query?.as_bytes())
        .find(|(key, _)| key == "token")
        .map(|(_, token)| token.trim().to_string())
        .filter(|token| !token.is_empty())
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;
    use crate::shared::Error;

    #[tokio::test]
    async fn an_sns_message_needs_its_signature_and_anything_else_is_left_to_the_format() {
        assert!(check_sns(b"not json").await.is_ok());
        assert!(check_sns(br#"{"AlarmName": "raw delivery"}"#).await.is_ok());

        // 证书只从 AWS 的地址取：别的地址不会去连。
        let forged = br#"{"Type": "Notification", "Message": "{}",
                          "SigningCertURL": "https://attacker.example.com/cert.pem"}"#;

        assert!(matches!(
            check_sns(forged).await,
            Err(Error::Unauthorized(_))
        ));

        let confirmation = br#"{"Type": "SubscriptionConfirmation",
                                "SubscribeURL": "https://attacker.example.com/confirm",
                                "SigningCertURL": "https://attacker.example.com/cert.pem"}"#;

        assert!(matches!(
            check_sns(confirmation).await,
            Err(Error::Unauthorized(_))
        ));
    }

    #[test]
    fn the_token_comes_from_the_header_or_the_address() {
        let mut headers = HeaderMap::new();
        assert_eq!(bearer(&headers), None);

        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer mspat-abc"));
        assert_eq!(bearer(&headers).as_deref(), Some("mspat-abc"));

        assert_eq!(
            query_token(Some("a=1&token=mspat-abc")).as_deref(),
            Some("mspat-abc")
        );
        assert_eq!(query_token(Some("token=")), None);
        assert_eq!(query_token(None), None);
    }
}
