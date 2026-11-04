// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 心跳规则：
//!
//! - `GET|POST /api/v1/intake/heartbeats/{token}`：打卡地址，不带登录，靠地址里的 Token。`?status=fail`
//!   报失败，马上触发；POST 的请求体开头记成这次打卡的话。Token 不对回 404。
//! - `GET /alerts/rules/{id}/heartbeat`：最后一次打卡、下一次最晚的时间；
//! - `POST /alerts/rules/{id}/heartbeat-token`：重置打卡地址，新地址只在这时给出一次。

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::AppState,
    app::{
        alerting::tokens::{new_token, token_hash},
        iam::IamContext,
    },
    domain::{
        alerting::{
            heartbeat::{HeartbeatRepository, HeartbeatStatus},
            rule::{AlertRule, AlertRuleKind},
        },
        iam::resource_permission,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const PUBLIC_PREFIX: &str = "/api/v1/intake/heartbeats/";

/// 打卡的请求体最多读多少字节。
const MAX_BODY_BYTES: usize = 64 * 1024;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/alerts/rules/{id}/heartbeat", get(heartbeat_state))
        .route("/alerts/rules/{id}/heartbeat-token", post(reset_token))
}

pub fn public_routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/intake/heartbeats/{token}",
        get(check_in)
            .post(check_in)
            .layer(DefaultBodyLimit::max(MAX_BODY_BYTES)),
    )
}

#[derive(Deserialize)]
struct CheckInQuery {
    #[serde(default)]
    status: Option<String>,
}

async fn check_in(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Query(query): Query<CheckInQuery>,
    body: Bytes,
) -> Response {
    let token = token.trim();

    if token.is_empty() || token.len() > 128 {
        return StatusCode::NOT_FOUND.into_response();
    }

    let status = HeartbeatStatus::parse(query.status.as_deref().unwrap_or("ok"));
    let message = String::from_utf8_lossy(&body);

    match state
        .alerting
        .evaluator
        .check_in(&token_hash(token), status, &message, TimestampMicros::now())
        .await
    {
        Ok(true) => Json(json!({ "ok": true, "status": status.as_str() })).into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::warn!(error = %error, "heartbeat check-in failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// 给心跳规则发出（或重置）打卡地址，返回 `{ path, url }`；地址只在这时给出一次。
pub(super) async fn issue_address(
    state: &AppState,
    heartbeats: &dyn HeartbeatRepository,
    rule: &AlertRule,
) -> Result<Value> {
    let token = new_token();

    heartbeats
        .replace_token(
            &rule.org_id,
            &rule.id,
            &token_hash(&token),
            TimestampMicros::now(),
        )
        .await?;

    let path = format!("{PUBLIC_PREFIX}{token}");
    let base = state.platform.external_url.trim().trim_end_matches('/');

    Ok(json!({
        "path": path,
        "url": (!base.is_empty()).then(|| format!("{base}{path}")),
    }))
}

#[resource_permission(
    action = "alerts.rules.read",
    resource = AlertRule,
    id = Id::from_string(id),
    bind = rule
)]
async fn heartbeat_state(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let Some(params) = rule.query.heartbeat.as_ref() else {
        return Err(Error::not_found("this rule is not a heartbeat rule"));
    };

    let beat = state
        .alerting
        .heartbeats
        .get(&rule.id)
        .await?
        .ok_or_else(|| Error::not_found("this heartbeat rule has no address yet"))?;

    let now = TimestampMicros::now();

    Ok(Json(json!({
        "lastCheckinAt": beat.last_checkin_at,
        "lastStatus": beat.last_status,
        "lastMessage": beat.last_message,
        "dueAt": beat.due_at(params),
        "alarming": beat.alarming(params, now),
        "issuedAt": beat.created_at,
    })))
}

#[resource_permission(
    action = "alerts.rules.manage",
    resource = AlertRule,
    id = Id::from_string(id),
    bind = rule
)]
async fn reset_token(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    if rule.kind != AlertRuleKind::Heartbeat {
        return Err(Error::invalid(
            "only a heartbeat rule has a check-in address",
        ));
    }

    let address = issue_address(&state, state.alerting.heartbeats.as_ref(), &rule).await?;

    crate::api::http::routes::activity_audit::record(
        &state,
        &ctx,
        "alert_rule.heartbeat_token_reset",
        "alert_rule",
        &rule.id.0,
        json!({ "name": rule.name }),
    )
    .await;

    Ok(Json(address))
}
