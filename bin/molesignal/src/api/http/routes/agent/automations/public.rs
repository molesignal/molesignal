// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! API 触发：`POST /api/v1/public/automations/{id}/trigger`。
//!
//! 外部系统（例如 CI/CD 发布之后）带 `Authorization: Bearer mspat-…` 调用，请求体可以带
//! `{"text": "…"}` 作为这一次的上下文。认证层放行 `/api/v1/public/automations/` 这个前缀，由这里
//! 用规则自己的令牌鉴权；令牌不对、规则不存在都一样回 401，不透露哪条规则存在。同一条规则
//! 十秒内只开始一次。

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    routing::post,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::start::{Start, start};
use crate::{
    agent::automations::{AutomationTrigger, MAX_INPUT_CHARS, TriggerKind},
    api::AppState,
    app::automations::token,
    domain::iam::api_token::has_token_marker,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 同一条规则两次 API 触发之间至少隔这么久。
const MIN_INTERVAL_MICROS: i64 = 10_000_000;

pub fn public_routes() -> Router<AppState> {
    Router::new().route("/api/v1/public/automations/{id}/trigger", post(trigger))
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct TriggerReq {
    #[serde(default)]
    text: Option<String>,
}

fn refused() -> Error {
    Error::unauthorized("invalid automation token")
}

async fn trigger(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>)> {
    let presented = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| has_token_marker(value))
        .ok_or_else(refused)?;

    let (automation, hash) = state
        .agent
        .automations
        .get_with_token(&Id(id))
        .await?
        .ok_or_else(refused)?;

    let hash = hash.ok_or_else(refused)?;

    if !token::matches(presented, &hash) {
        return Err(refused());
    }

    if !automation.enabled {
        return Err(Error::conflict("this automation is disabled"));
    }

    if !matches!(automation.trigger, AutomationTrigger::Api) {
        return Err(Error::conflict(
            "this automation is not started through its API",
        ));
    }

    let request: TriggerReq = if body.iter().all(u8::is_ascii_whitespace) {
        TriggerReq::default()
    } else {
        serde_json::from_slice(&body)
            .map_err(|error| Error::invalid(format!("invalid request body: {error}")))?
    };

    let text = request.text.map(|text| text.trim().to_string());

    if text
        .as_ref()
        .is_some_and(|text| text.chars().count() > MAX_INPUT_CHARS)
    {
        return Err(Error::invalid(format!(
            "text has at most {MAX_INPUT_CHARS} characters"
        )));
    }

    let recently = automation
        .last_run
        .as_ref()
        .is_some_and(|run| TimestampMicros::now().0 - run.at.0 < MIN_INTERVAL_MICROS);

    if recently {
        return Err(Error::ResourceExhausted(
            "this automation was started a moment ago; try again in a few seconds".into(),
        ));
    }

    let run = start(
        &state,
        &automation,
        Start {
            trigger: TriggerKind::Api,
            text: text.filter(|text| !text.is_empty()),
            incident: None,
            finding: None,
            started_by: None,
        },
    )
    .await?;

    let url = run.chat_id.as_ref().map(|chat_id| {
        format!(
            "{}/agent/chat?chat={}",
            state.platform.external_url.trim_end_matches('/'),
            chat_id.0
        )
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "runId": run.id.0,
            "status": run.status.as_str(),
            "reason": run.reason,
            "chatUrl": url,
        })),
    ))
}
