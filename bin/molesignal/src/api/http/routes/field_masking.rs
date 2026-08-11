// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 组织级字段遮掩规则与单流有效配置。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, put},
};
use serde::Deserialize;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        iam::{permission, resource_permission},
        masking::{
            EffectiveFieldMasking, FieldMaskingAlgorithm, FieldMaskingProvider, FieldMaskingRule,
        },
        stream::{StreamDefinition, StreamType},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/field_masking/rules", get(list).post(create))
        .route("/field_masking/rules/reorder", put(reorder))
        .route("/field_masking/rules/{id}", put(update).delete(delete))
        .route("/field_masking/effective/{stream_id}", get(effective))
}

#[derive(Debug, Deserialize)]
struct RuleRequest {
    name: String,
    #[serde(default = "default_true")]
    enabled: bool,
    field_pattern: String,
    #[serde(default)]
    stream_pattern: Option<String>,
    #[serde(default)]
    stream_type: Option<StreamType>,
    algorithm: FieldMaskingAlgorithm,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct ReorderRequest {
    ids: Vec<Id>,
}

fn normalize_request(request: RuleRequest) -> Result<RuleRequest> {
    let name = request.name.trim().to_string();
    let field_pattern = request.field_pattern.trim().to_string();
    if name.is_empty() || name.len() > 255 {
        return Err(Error::invalid("name must be 1..255 characters"));
    }
    if field_pattern.is_empty() || field_pattern.len() > 255 {
        return Err(Error::invalid("field_pattern must be 1..255 characters"));
    }
    let stream_pattern = request
        .stream_pattern
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if stream_pattern
        .as_ref()
        .is_some_and(|value| value.len() > 255)
    {
        return Err(Error::invalid(
            "stream_pattern must not exceed 255 characters",
        ));
    }
    if request.stream_type == Some(StreamType::Metrics) {
        return Err(Error::invalid(
            "metrics streams do not support field masking",
        ));
    }
    request.algorithm.validate()?;
    Ok(RuleRequest {
        name,
        field_pattern,
        stream_pattern,
        ..request
    })
}

#[permission("org.settings.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<FieldMaskingRule>>> {
    Ok(Json(
        state.storage.field_masking_rules.list(&ctx.org_id).await?,
    ))
}

#[permission("org.settings.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<RuleRequest>,
) -> Result<Json<FieldMaskingRule>> {
    let request = normalize_request(request)?;
    let now = TimestampMicros::now();
    let rule = FieldMaskingRule {
        id: Id::new(),
        org_id: ctx.org_id,
        name: request.name,
        // Repository 在组织级事务锁内分配最终优先级，避免并发创建碰撞。
        priority: 0,
        enabled: request.enabled,
        field_pattern: request.field_pattern,
        stream_pattern: request.stream_pattern,
        stream_type: request.stream_type,
        algorithm: request.algorithm,
        created_at: now,
        updated_at: now,
    };
    Ok(Json(state.storage.field_masking_rules.create(rule).await?))
}

#[permission("org.settings.manage")]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<RuleRequest>,
) -> Result<Json<FieldMaskingRule>> {
    let request = normalize_request(request)?;
    let existing = state.storage.field_masking_rules.list(&ctx.org_id).await?;
    let current = existing
        .into_iter()
        .find(|rule| rule.id.0 == id)
        .ok_or_else(|| Error::not_found("field masking rule"))?;
    let rule = FieldMaskingRule {
        id: current.id,
        org_id: ctx.org_id,
        name: request.name,
        priority: current.priority,
        enabled: request.enabled,
        field_pattern: request.field_pattern,
        stream_pattern: request.stream_pattern,
        stream_type: request.stream_type,
        algorithm: request.algorithm,
        created_at: current.created_at,
        updated_at: TimestampMicros::now(),
    };
    Ok(Json(state.storage.field_masking_rules.update(rule).await?))
}

#[permission("org.settings.manage")]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    state
        .storage
        .field_masking_rules
        .delete(&ctx.org_id, &Id::from_string(id))
        .await?;
    Ok(Json(serde_json::json!({"deleted": true})))
}

#[permission("org.settings.manage")]
async fn reorder(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<ReorderRequest>,
) -> Result<Json<Vec<FieldMaskingRule>>> {
    state
        .storage
        .field_masking_rules
        .reorder(&ctx.org_id, &request.ids, TimestampMicros::now())
        .await?;
    Ok(Json(
        state.storage.field_masking_rules.list(&ctx.org_id).await?,
    ))
}

#[resource_permission(
    action = any("streams.read", "sys.telemetry.read"),
    resource = StreamDefinition,
    id = Id::from_string(stream_id),
    bind = stream
)]
async fn effective(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(stream_id): Path<String>,
) -> Result<Json<EffectiveFieldMasking>> {
    Ok(Json(
        state
            .storage
            .field_masking
            .effective_for_stream(&stream.org_id, &stream.id)
            .await?,
    ))
}
