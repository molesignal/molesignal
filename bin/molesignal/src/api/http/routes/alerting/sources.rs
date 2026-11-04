// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警来源路由：外部系统把告警发进来时的身份与配置。
//!
//! - `GET    /api/v1/alerts/sources`                  列表（`alerts.sources.read`）
//! - `POST   /api/v1/alerts/sources`                  新建，响应里带一次 Token（`alerts.sources.manage`）
//! - `GET    /api/v1/alerts/sources/clusters`         接收集群可以选哪些（`alerts.sources.read`）
//! - `GET    /api/v1/alerts/sources/{id}`             详情（`alerts.sources.read`）
//! - `PUT    /api/v1/alerts/sources/{id}`             修改（`alerts.sources.manage`）
//! - `DELETE /api/v1/alerts/sources/{id}`             删除，并解决它的活跃事件（`alerts.sources.manage`）
//! - `POST   /api/v1/alerts/sources/{id}/token`       轮换 Token，旧的立即失效（`alerts.sources.manage`）
//! - `GET    /api/v1/alerts/sources/{id}/requests`    最近收到的请求（`alerts.sources.read`）
//! - `GET    /api/v1/alerts/incidents/{id}/source`    外部事件的来源与分组里的告警（`alerts.incidents.read`）
//!
//! 来源随联邦同步（Token 的哈希一起），接口从不返回哈希。

use std::collections::BTreeMap;

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::intake::INTAKE_PATH;
use crate::{
    api::{
        AppState,
        http::{
            federation::{delete_payload, emit_cud},
            routes::activity_audit,
        },
    },
    app::{alerting::sources::SourceWrite, iam::IamContext},
    domain::{
        alerting::{
            incident::{Incident, Severity},
            source::{AlertSource, AlertSourceKind, EmailRules},
        },
        federation::{CudAction, ResourceKind},
        iam::{permission, resource_permission},
    },
    shared::{Error, Result, ids::Id},
};

/// 最近收到的请求，一次最多给几条。
const MAX_REQUESTS_PAGE: u32 = 200;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/alerts/sources", get(list).post(create))
        .route("/alerts/sources/clusters", get(clusters))
        .route(
            "/alerts/sources/{id}",
            get(get_one).put(update).delete(delete),
        )
        .route(
            "/alerts/sources/{id}/token",
            axum::routing::post(rotate_token),
        )
        .route("/alerts/sources/{id}/requests", get(requests))
        .route("/alerts/incidents/{id}/source", get(incident_source))
}

fn yes() -> bool {
    true
}

fn warning() -> Severity {
    Severity::Warning
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SourceReq {
    /// 只在新建时写；修改时带了也必须和原来的一样。
    #[serde(default)]
    kind: Option<AlertSourceKind>,
    name: String,
    #[serde(default = "yes")]
    enabled: bool,
    #[serde(default)]
    escalation_policy_id: Option<Id>,
    #[serde(default = "warning")]
    severity: Severity,
    #[serde(default)]
    labels: BTreeMap<String, String>,
    #[serde(default)]
    home_cluster: Option<String>,
    #[serde(default)]
    email: Option<EmailRules>,
}

impl SourceReq {
    fn into_write(self) -> SourceWrite {
        SourceWrite {
            name: self.name,
            enabled: self.enabled,
            escalation_policy_id: self
                .escalation_policy_id
                .filter(|id| !id.0.trim().is_empty()),
            severity: self.severity,
            labels: self.labels,
            home_cluster: self.home_cluster,
            email: self.email,
        }
    }
}

/// 接口里的来源：不带 Token 的哈希，带上接入地址的路径和列表里显示的 Token 前缀。
fn view(source: &AlertSource) -> Value {
    json!({
        "id": source.id,
        "name": source.name,
        "kind": source.kind,
        "enabled": source.enabled,
        "escalationPolicyId": source.escalation_policy_id,
        "severity": source.severity,
        "labels": source.labels,
        "tokenPrefix": source.shown_token(),
        "homeCluster": source.home_cluster,
        "intakePath": INTAKE_PATH,
        "createdBy": source.created_by,
        "createdAt": source.created_at.0,
        "updatedAt": source.updated_at.0,
        "lastIntakeAt": source.last_intake_at.map(|at| at.0),
        "email": source.email,
    })
}

/// 带一次 Token 的来源：新建与轮换时。
fn view_with_token(source: &AlertSource, token: &str) -> Value {
    let mut value = view(source);
    value["token"] = Value::String(token.to_string());
    value
}

async fn audit(state: &AppState, ctx: &IamContext, action: &str, source: &AlertSource) {
    activity_audit::record(
        state,
        ctx,
        action,
        "alert_source",
        &source.id.0,
        json!({ "name": source.name, "kind": source.kind, "tokenPrefix": source.shown_token() }),
    )
    .await;
}

#[permission("alerts.sources.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let sources = state.alerting.sources.list(&ctx.org_id).await?;
    Ok(Json(Value::Array(sources.iter().map(view).collect())))
}

#[permission("alerts.sources.read")]
async fn clusters(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let local = state.alerting.sources.local_cluster().await?;
    let clusters = state.alerting.sources.clusters().await?;
    Ok(Json(json!({ "local": local, "clusters": clusters })))
}

#[permission("alerts.sources.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<SourceReq>,
) -> Result<Json<Value>> {
    let kind = req.kind.ok_or_else(|| Error::invalid("kind is required"))?;

    let (source, token) = state
        .alerting
        .sources
        .create(&ctx.org_id, &ctx.user_id, kind, req.into_write())
        .await?;

    emit_cud(
        &state,
        &source.org_id,
        ResourceKind::AlertSource,
        CudAction::Created,
        &source.id.0,
        &source,
    )
    .await;

    audit(&state, &ctx, "alerts.source.created", &source).await;
    Ok(Json(view_with_token(&source, &token)))
}

#[permission("alerts.sources.read")]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let source = state.alerting.sources.get(&ctx.org_id, &Id(id)).await?;
    Ok(Json(view(&source)))
}

#[permission("alerts.sources.manage")]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<SourceReq>,
) -> Result<Json<Value>> {
    let id = Id(id);
    let current = state.alerting.sources.get(&ctx.org_id, &id).await?;

    if req.kind.is_some_and(|kind| kind != current.kind) {
        return Err(Error::invalid("the kind of an alert source cannot change"));
    }

    let source = state
        .alerting
        .sources
        .update(&ctx.org_id, &id, req.into_write())
        .await?;

    emit_cud(
        &state,
        &source.org_id,
        ResourceKind::AlertSource,
        CudAction::Updated,
        &source.id.0,
        &source,
    )
    .await;

    audit(&state, &ctx, "alerts.source.updated", &source).await;
    Ok(Json(view(&source)))
}

#[permission("alerts.sources.manage")]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<&'static str> {
    let source = state
        .alerting
        .sources
        .delete(&ctx.org_id, &Id(id), &ctx.user_id)
        .await?;

    emit_cud(
        &state,
        &source.org_id,
        ResourceKind::AlertSource,
        CudAction::Deleted,
        &source.id.0,
        &delete_payload(&source.id.0),
    )
    .await;

    audit(&state, &ctx, "alerts.source.deleted", &source).await;
    Ok("deleted")
}

#[permission("alerts.sources.manage")]
async fn rotate_token(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let (source, token) = state
        .alerting
        .sources
        .rotate_token(&ctx.org_id, &Id(id))
        .await?;

    emit_cud(
        &state,
        &source.org_id,
        ResourceKind::AlertSource,
        CudAction::Updated,
        &source.id.0,
        &source,
    )
    .await;

    audit(&state, &ctx, "alerts.source.token_rotated", &source).await;
    Ok(Json(view_with_token(&source, &token)))
}

#[derive(Debug, Deserialize)]
struct RequestsQuery {
    #[serde(default)]
    limit: Option<u32>,
}

#[permission("alerts.sources.read")]
async fn requests(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Query(query): Query<RequestsQuery>,
) -> Result<Json<Value>> {
    let limit = query.limit.unwrap_or(50).clamp(1, MAX_REQUESTS_PAGE);

    let requests = state
        .alerting
        .sources
        .requests(&ctx.org_id, &Id(id), limit)
        .await?;

    let items: Vec<Value> = requests
        .iter()
        .map(|request| {
            json!({
                "id": request.id,
                "receivedAt": request.received_at.0,
                "viaCluster": request.via_cluster,
                "status": request.status,
                "httpStatus": request.http_status,
                "message": request.message,
                "alertCount": request.alert_count,
                "alerts": request.alerts,
                "bodyExcerpt": request.body_excerpt,
            })
        })
        .collect();

    Ok(Json(Value::Array(items)))
}

#[resource_permission(
    action = "alerts.incidents.read",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn incident_source(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let found = state
        .alerting
        .sources
        .incident_source(&incident)
        .await?
        .ok_or_else(|| Error::not_found("this incident was not opened by an alert source"))?;

    Ok(Json(json!({
        "source": found.source.as_ref().map(view),
        "dedupKey": found.state.dedup_key,
        "lastEventAt": found.state.last_event_at.map(|at| at.0),
        "lastIntakeAt": found.state.last_intake_at.0,
        "members": found.state.members,
    })))
}
