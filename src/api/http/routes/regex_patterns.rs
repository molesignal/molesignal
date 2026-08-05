// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Regex pattern CRUD（backend-settings-endpoints）。
//!
//! 兼作敏感数据脱敏规则：`replacement`（命中替换串，支持 `$1` 捕获组）+ `apply_on_ingest`
//! （写入即脱敏）。查询端 `mask(col)` 应用本表全部规则。CUD 后失效写入端 masker 缓存。
//!
//! `GET    /regex_patterns`       list（per-org）
//! `POST   /regex_patterns`       create（pattern 校验 by `regex::Regex::new`）
//! `PUT    /regex_patterns/{id}`  update
//! `DELETE /regex_patterns/{id}`  delete

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::Deserialize;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        federation::{CudAction, ResourceKind},
        iam::permission,
    },
    infra::persistence::repositories::regex_patterns::RegexPattern,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/regex_patterns", get(list).post(create))
        .route(
            "/regex_patterns/{id}",
            axum::routing::put(update).delete(delete),
        )
}

#[derive(Debug, Deserialize)]
pub struct CreateReq {
    pub name: String,
    pub pattern: String,
    #[serde(default)]
    pub description: Option<String>,
    /// 命中片段替换成的串（支持 `$1` 捕获组）；缺省 `[REDACTED]`，空串 = 删除命中片段。
    #[serde(default)]
    pub replacement: Option<String>,
    /// 写入前对所有字符串值做不可逆脱敏；缺省 false（仅查询端 `mask(col)` 应用）。
    #[serde(default)]
    pub apply_on_ingest: Option<bool>,
}

fn replacement_or_default(r: Option<String>) -> String {
    r.unwrap_or_else(|| "[REDACTED]".to_string())
}

#[permission("org.settings.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<RegexPattern>>> {
    let rows = state.storage.regex_patterns.list(&ctx.org_id).await?;
    Ok(Json(rows))
}

#[permission("org.settings.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateReq>,
) -> Result<Json<RegexPattern>> {
    if req.name.trim().is_empty() {
        return Err(Error::invalid("name must not be empty"));
    }
    regex::Regex::new(&req.pattern).map_err(|e| Error::invalid(format!("invalid regex: {e}")))?;
    let existing = state.storage.regex_patterns.list(&ctx.org_id).await?;
    if existing.iter().any(|p| p.name == req.name) {
        return Err(Error::conflict("pattern name already exists"));
    }
    let now = TimestampMicros::now();
    let p = RegexPattern {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        name: req.name,
        pattern: req.pattern,
        description: req.description.unwrap_or_default(),
        replacement: replacement_or_default(req.replacement),
        apply_on_ingest: req.apply_on_ingest.unwrap_or(false),
        created_at: now,
        updated_at: now,
    };
    let saved = state.storage.regex_patterns.create(p).await?;
    state.storage.masking.invalidate(&ctx.org_id).await;
    crate::api::http::federation::emit_cud(
        &state,
        &ctx.org_id,
        ResourceKind::RegexPattern,
        CudAction::Created,
        &saved.id.0,
        &saved,
    )
    .await;
    Ok(Json(saved))
}

#[permission("org.settings.manage")]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<CreateReq>,
) -> Result<Json<RegexPattern>> {
    if req.name.trim().is_empty() {
        return Err(Error::invalid("name must not be empty"));
    }
    regex::Regex::new(&req.pattern).map_err(|e| Error::invalid(format!("invalid regex: {e}")))?;
    let existing = state.storage.regex_patterns.list(&ctx.org_id).await?;
    // name 唯一性：排除自身。
    if existing.iter().any(|p| p.name == req.name && p.id.0 != id) {
        return Err(Error::conflict("pattern name already exists"));
    }
    let created_at = existing
        .iter()
        .find(|p| p.id.0 == id)
        .map(|p| p.created_at)
        .ok_or_else(|| Error::not_found("regex pattern"))?;
    let p = RegexPattern {
        id: Id(id),
        org_id: ctx.org_id.clone(),
        name: req.name,
        pattern: req.pattern,
        description: req.description.unwrap_or_default(),
        replacement: replacement_or_default(req.replacement),
        apply_on_ingest: req.apply_on_ingest.unwrap_or(false),
        created_at,
        updated_at: TimestampMicros::now(),
    };
    let saved = state.storage.regex_patterns.update(p).await?;
    state.storage.masking.invalidate(&ctx.org_id).await;
    crate::api::http::federation::emit_cud(
        &state,
        &ctx.org_id,
        ResourceKind::RegexPattern,
        CudAction::Updated,
        &saved.id.0,
        &saved,
    )
    .await;
    Ok(Json(saved))
}

#[permission("org.settings.manage")]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    state
        .storage
        .regex_patterns
        .delete(&ctx.org_id, &Id(id.clone()))
        .await?;
    state.storage.masking.invalidate(&ctx.org_id).await;
    crate::api::http::federation::emit_cud(
        &state,
        &ctx.org_id,
        ResourceKind::RegexPattern,
        CudAction::Deleted,
        &id,
        &crate::api::http::federation::delete_payload(&id),
    )
    .await;
    Ok(Json(serde_json::json!({"deleted": true})))
}
