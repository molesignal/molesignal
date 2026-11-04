// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 企业微信自建应用的「接收消息」回调：`GET|POST /api/v1/public/notify/{connector_id}/wecom`。
//!
//! 应用后台「接收消息」的 URL 填这个地址（连接器页显示它），Token 与 EncodingAESKey 填进连接器。保存时
//! 企业微信用 GET 校验地址：验过签名、解开 `echostr`，回它的明文。之后的事件 POST 过来：验签、解密，只处理
//! 模板卡片按钮的点击，另起任务处理，马上回 `success`（企业微信只等 5 秒）；别的事件照样回 `success`。
//!
//! 连接器停用了、不是自建应用或没配回调的，一律 404；签名不对 401。

use axum::{
    Router,
    body::Bytes,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;

use crate::{
    api::{AppState, http::routes::agent::im::wecom_app_press},
    domain::notify::connector::NotifyConnector,
    infra::notify::adapters::{
        WECOM_APP_CONNECTOR_TYPE, WecomCallback, wecom_card_event, wecom_encrypted_of,
    },
    shared::{Error, Result, ids::Id},
};

pub(super) fn public_routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/public/notify/{connectorId}/wecom",
        get(verify).post(event),
    )
}

/// 企业微信给每个请求带上的签名参数；校验地址时还有 `echostr`。
#[derive(Debug, Deserialize)]
struct Signed {
    msg_signature: String,
    timestamp: String,
    nonce: String,
    #[serde(default)]
    echostr: Option<String>,
}

async fn callback_of(
    state: &AppState,
    connector_id: &str,
) -> Result<(NotifyConnector, WecomCallback)> {
    let missing = || Error::not_found("no such callback");

    let connector = state
        .alerting
        .reply_callbacks
        .connector(&Id::from_string(connector_id))
        .await?
        .filter(|connector| {
            connector.enabled && connector.connector_type == WECOM_APP_CONNECTOR_TYPE
        })
        .ok_or_else(missing)?;

    let callback = WecomCallback::of(&connector.config)
        .ok()
        .flatten()
        .ok_or_else(missing)?;

    Ok((connector, callback))
}

/// 校验地址：解开 `echostr`，回它的明文。
async fn verify(
    State(state): State<AppState>,
    Path(connector_id): Path<String>,
    Query(signed): Query<Signed>,
) -> Result<Response> {
    let (_, callback) = callback_of(&state, &connector_id).await?;
    let echo = signed
        .echostr
        .as_deref()
        .ok_or_else(|| Error::invalid("echostr is required"))?;

    if !callback.signed(
        &signed.timestamp,
        &signed.nonce,
        echo,
        &signed.msg_signature,
    ) {
        return Err(Error::unauthorized("invalid WeCom signature"));
    }

    Ok(callback.decrypt(echo)?.into_response())
}

/// 一个事件：模板卡片按钮的点击另起任务处理，都马上回 `success`。
async fn event(
    State(state): State<AppState>,
    Path(connector_id): Path<String>,
    Query(signed): Query<Signed>,
    body: Bytes,
) -> Result<Response> {
    let (connector, callback) = callback_of(&state, &connector_id).await?;

    let body = std::str::from_utf8(&body).map_err(|_| Error::invalid("the body is not XML"))?;
    let encrypted =
        wecom_encrypted_of(body).ok_or_else(|| Error::invalid("the body has no Encrypt"))?;

    if !callback.signed(
        &signed.timestamp,
        &signed.nonce,
        &encrypted,
        &signed.msg_signature,
    ) {
        return Err(Error::unauthorized("invalid WeCom signature"));
    }

    if let Some(press) = wecom_card_event(&callback.decrypt(&encrypted)?) {
        tokio::spawn(wecom_app_press(state, connector, press));
    }

    Ok("success".into_response())
}
