// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 邮件来源读的 IMAP 邮箱：
//!
//! - `GET    /api/v1/alerts/sources/{id}/mailbox`        设置和上次读的结果，没有秘密；没配时为 `null`（`alerts.sources.read`）
//! - `PUT    /api/v1/alerts/sources/{id}/mailbox`        配邮箱；`secret` 留空时保持原样（`alerts.sources.manage`）
//! - `DELETE /api/v1/alerts/sources/{id}/mailbox`        不再读邮箱（`alerts.sources.manage`）
//! - `POST   /api/v1/alerts/sources/{id}/mailbox/test`   试连表单里的设置（`alerts.sources.manage`）
//!
//! 邮箱只配在来源的接收集群上，不随联邦同步。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{alerting::sources::MailboxWrite, iam::IamContext},
    domain::{
        alerting::source::{Mailbox, MailboxSettings},
        iam::permission,
    },
    shared::{Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/alerts/sources/{id}/mailbox",
            get(read).put(save).delete(remove),
        )
        .route("/alerts/sources/{id}/mailbox/test", post(test))
}

#[derive(Debug, Deserialize)]
struct MailboxReq {
    #[serde(flatten)]
    settings: MailboxSettings,
    /// 密码或客户端密钥；留空保持原样。
    #[serde(default)]
    secret: Option<String>,
}

impl From<MailboxReq> for MailboxWrite {
    fn from(req: MailboxReq) -> Self {
        Self {
            settings: req.settings,
            secret: req.secret,
        }
    }
}

/// 给页面的邮箱：不带秘密，说明设了没有。
fn view(mailbox: &Mailbox) -> Value {
    json!({
        "settings": mailbox.settings,
        "secretSet": !mailbox.secret.is_empty(),
        "reading": mailbox.cursor.is_some(),
        "lastPolledAt": mailbox.last_polled_at.map(|at| at.0),
        "lastSuccessAt": mailbox.last_success_at.map(|at| at.0),
        "lastError": mailbox.last_error,
        "updatedAt": mailbox.updated_at.0,
    })
}

async fn audit(state: &AppState, ctx: &IamContext, action: &str, id: &str, detail: Value) {
    activity_audit::record(state, ctx, action, "alert_source", id, detail).await;
}

#[permission("alerts.sources.read")]
async fn read(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let mailbox = state.alerting.sources.mailbox(&ctx.org_id, &Id(id)).await?;

    Ok(Json(mailbox.as_ref().map_or(Value::Null, view)))
}

#[permission("alerts.sources.manage")]
async fn save(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<MailboxReq>,
) -> Result<Json<Value>> {
    let mailbox = state
        .alerting
        .sources
        .save_mailbox(&ctx.org_id, &Id(id.clone()), req.into())
        .await?;

    audit(
        &state,
        &ctx,
        "alerts.source.mailbox_saved",
        &id,
        json!({
            "host": mailbox.settings.host,
            "username": mailbox.settings.username,
            "folder": mailbox.settings.folder,
        }),
    )
    .await;

    Ok(Json(view(&mailbox)))
}

#[permission("alerts.sources.manage")]
async fn remove(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    state
        .alerting
        .sources
        .delete_mailbox(&ctx.org_id, &Id(id.clone()))
        .await?;

    audit(
        &state,
        &ctx,
        "alerts.source.mailbox_removed",
        &id,
        json!({}),
    )
    .await;
    Ok(Json(json!({ "deleted": true })))
}

#[permission("alerts.sources.manage")]
async fn test(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<MailboxReq>,
) -> Result<Json<Value>> {
    let status = state
        .alerting
        .sources
        .check_mailbox(&ctx.org_id, &Id(id), req.into())
        .await?;

    Ok(Json(json!({ "ok": true, "messages": status.messages })))
}
