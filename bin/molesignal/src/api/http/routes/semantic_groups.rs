// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警分组规则（semantic groups）HTTP 路由：CRUD + 批量导入。
//!
//! 权限复用告警域：读 `AlertRead`、写 `AlertWrite`。所有操作按 `ctx.org_id` 隔离
//! （repo 的 get/update/delete 仅按 id，故 by-id 路径 load 后校验 org 归属）。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{
        AppState,
        http::federation::{delete_payload, emit_cud},
    },
    app::iam::IamContext,
    domain::{
        alerting::semantic_group::{LabelMatcher, SemanticGroup},
        federation::{CudAction, ResourceKind},
        iam::permission,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/alerts/semantic_groups", get(list).post(create))
        .route("/alerts/semantic_groups/import", post(import))
        .route(
            "/alerts/semantic_groups/{id}",
            get(get_one).put(update).delete(delete_one),
        )
}

#[derive(Debug, Deserialize)]
struct GroupWriteReq {
    name: String,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    matchers: Vec<LabelMatcher>,
    #[serde(default)]
    group_by: Vec<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct ImportReq {
    groups: Vec<GroupWriteReq>,
}

#[derive(Debug, Serialize)]
struct ImportResp {
    imported: usize,
}

/// 校验写请求并组装 [`SemanticGroup`]。`id`/`org_id`/时间戳由 caller 决定。
fn validate(req: &GroupWriteReq) -> Result<()> {
    if req.name.trim().is_empty() {
        return Err(Error::invalid("name cannot be empty"));
    }
    if req.name.len() > 255 {
        return Err(Error::invalid("name must be 255 chars or fewer"));
    }
    for m in &req.matchers {
        if m.label.trim().is_empty() {
            return Err(Error::invalid("matcher.label cannot be empty"));
        }
    }
    if req.group_by.iter().any(|k| k.trim().is_empty()) {
        return Err(Error::invalid("group_by keys cannot be empty"));
    }
    Ok(())
}

fn build(req: GroupWriteReq, id: Id, org_id: Id, now: TimestampMicros) -> SemanticGroup {
    SemanticGroup {
        id,
        org_id,
        name: req.name,
        enabled: req.enabled,
        matchers: req.matchers,
        group_by: req.group_by,
        created_at: now,
        updated_at: now,
    }
}

#[permission("alerts.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<SemanticGroup>>> {
    Ok(Json(
        state.alerting.semantic_groups.list(&ctx.org_id).await?,
    ))
}

#[permission("alerts.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<GroupWriteReq>,
) -> Result<Json<SemanticGroup>> {
    validate(&req)?;
    let now = TimestampMicros::now();
    let group = build(req, Id::new(), ctx.org_id.clone(), now);
    let saved = state.alerting.semantic_groups.create(group).await?;
    emit_cud(
        &state,
        &ctx.org_id,
        ResourceKind::SemanticGroup,
        CudAction::Created,
        &saved.id.0,
        &saved,
    )
    .await;
    Ok(Json(saved))
}

#[permission("alerts.read")]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<SemanticGroup>> {
    let g = state
        .alerting
        .semantic_groups
        .get(&Id::from_string(id))
        .await?;
    if g.org_id != ctx.org_id {
        return Err(Error::forbidden("semantic group belongs to another org"));
    }
    Ok(Json(g))
}

#[permission("alerts.manage")]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<GroupWriteReq>,
) -> Result<Json<SemanticGroup>> {
    validate(&req)?;
    let existing = state
        .alerting
        .semantic_groups
        .get(&Id::from_string(id))
        .await?;
    if existing.org_id != ctx.org_id {
        return Err(Error::forbidden("semantic group belongs to another org"));
    }
    let group = SemanticGroup {
        created_at: existing.created_at,
        ..build(req, existing.id, ctx.org_id.clone(), TimestampMicros::now())
    };
    let saved = state.alerting.semantic_groups.update(group).await?;
    emit_cud(
        &state,
        &ctx.org_id,
        ResourceKind::SemanticGroup,
        CudAction::Updated,
        &saved.id.0,
        &saved,
    )
    .await;
    Ok(Json(saved))
}

#[permission("alerts.manage")]
async fn delete_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<&'static str> {
    let existing = state
        .alerting
        .semantic_groups
        .get(&Id::from_string(id))
        .await?;
    if existing.org_id != ctx.org_id {
        return Err(Error::forbidden("semantic group belongs to another org"));
    }
    state.alerting.semantic_groups.delete(&existing.id).await?;
    emit_cud(
        &state,
        &ctx.org_id,
        ResourceKind::SemanticGroup,
        CudAction::Deleted,
        &existing.id.0,
        &delete_payload(&existing.id.0),
    )
    .await;
    Ok("deleted")
}

/// 批量导入（append 语义，不删现有）：先整体校验（任一条非法 → 400，不写任何一条），
/// 再经 `create_many` 在单事务内创建（中途 DB 失败整体回滚，不留半截导入）。
#[permission("alerts.manage")]
async fn import(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<ImportReq>,
) -> Result<Json<ImportResp>> {
    if req.groups.is_empty() {
        return Err(Error::invalid("groups cannot be empty"));
    }
    for g in &req.groups {
        validate(g)?;
    }
    let now = TimestampMicros::now();
    let groups: Vec<_> = req
        .groups
        .into_iter()
        .map(|g| build(g, Id::new(), ctx.org_id.clone(), now))
        .collect();
    let imported = state
        .alerting
        .semantic_groups
        .create_many(groups.clone())
        .await?;
    // 批量导入逐条传播（事务成功后才发，避免回滚的半截事件）。
    for g in &groups {
        emit_cud(
            &state,
            &ctx.org_id,
            ResourceKind::SemanticGroup,
            CudAction::Created,
            &g.id.0,
            g,
        )
        .await;
    }
    Ok(Json(ImportResp { imported }))
}
