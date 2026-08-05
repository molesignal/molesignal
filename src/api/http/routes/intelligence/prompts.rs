// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 指令模板路由。
//!
//! 路径统一位于 `/api/v1/intelligence/settings/prompts`。
//!
//! builtin 行只读。变量校验：body 只能引用 `variables_schema` 允许的变量（task 2.4）。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    api::{
        AppState,
        http::{middleware::Permission, routes::activity_audit},
    },
    app::iam::IamContext,
    domain::iam::permission,
    infra::persistence::repositories::intelligence::prompts::{
        AgentPromptTemplate, prompt_hash, validate_template_variables,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/intelligence/settings/prompts", get(list).post(create))
        .route(
            "/intelligence/settings/prompts/{id}",
            axum::routing::put(update).delete(delete),
        )
        .route(
            "/intelligence/settings/prompts/{id}/set-default",
            axum::routing::post(set_default),
        )
        .route(
            "/intelligence/settings/prompts/{id}/restore",
            axum::routing::post(restore),
        )
}

fn require_license(state: &AppState) -> Result<()> {
    if !state
        .platform
        .license
        .has_feature(crate::intelligence::FEATURE)
    {
        return Err(Error::forbidden("intelligence feature not licensed"));
    }
    Ok(())
}

const PURPOSES: &[&str] = &[
    "system",
    "anomaly_analysis",
    "root_cause",
    "alert_explain",
    "query_generation",
    "dashboard_authoring",
];

fn validate_purpose(p: &str) -> Result<()> {
    if PURPOSES.contains(&p) {
        Ok(())
    } else {
        Err(Error::invalid(format!("unknown purpose: {p}")))
    }
}

/// scope-based 授权：org override 需 Admin+；user override 需 StreamWrite + 归属本人。
fn authorize_scope(ctx: &IamContext, scope: &str, owner_user_id: Option<&str>) -> Result<()> {
    match scope {
        "org" => Permission::require_key(ctx, "intelligence.manage"),
        "user" => {
            Permission::require_key(ctx, "intelligence.manage")?;
            if let Some(uid) = owner_user_id
                && uid != ctx.user_id.0
            {
                return Err(Error::forbidden("cannot modify another user's prompt"));
            }
            Ok(())
        }
        "builtin" => Err(Error::invalid("builtin prompts are immutable")),
        other => Err(Error::invalid(format!("unknown scope: {other}"))),
    }
}

#[derive(Debug, Serialize)]
pub struct PromptResp {
    pub id: String,
    pub org_id: Option<String>,
    pub user_id: Option<String>,
    pub scope: String,
    pub builtin_key: Option<String>,
    pub purpose: String,
    pub name: String,
    pub body: String,
    pub variables_schema: Value,
    pub is_default: bool,
    pub enabled: bool,
    pub version: i32,
    pub parent_id: Option<String>,
    pub created_at_micros: i64,
    pub updated_at_micros: i64,
}

fn to_resp(t: AgentPromptTemplate) -> PromptResp {
    PromptResp {
        id: t.id.0,
        org_id: t.org_id,
        user_id: t.user_id,
        scope: t.scope,
        builtin_key: t.builtin_key,
        purpose: t.purpose,
        name: t.name,
        body: t.body,
        variables_schema: t.variables_schema,
        is_default: t.is_default,
        enabled: t.enabled,
        version: t.version,
        parent_id: t.parent_id,
        created_at_micros: t.created_at.0,
        updated_at_micros: t.updated_at.0,
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateReq {
    pub scope: String, // org | user
    pub purpose: String,
    #[serde(default)]
    pub builtin_key: Option<String>,
    pub name: String,
    pub body: String,
    #[serde(default)]
    pub variables_schema: Option<Value>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateReq {
    pub name: String,
    pub body: String,
    #[serde(default)]
    pub variables_schema: Option<Value>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[permission("intelligence.use")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<PromptResp>>> {
    require_license(&state)?;
    let rows = state
        .intelligence
        .prompts
        .list(&ctx.org_id, &ctx.user_id)
        .await?;
    Ok(Json(rows.into_iter().map(to_resp).collect()))
}

async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateReq>,
) -> Result<Json<PromptResp>> {
    require_license(&state)?;
    validate_purpose(&req.purpose)?;
    if req.name.trim().is_empty() || req.body.trim().is_empty() {
        return Err(Error::invalid("name and body are required"));
    }
    // user scope 归属本人；org scope owner=None。
    authorize_scope(&ctx, &req.scope, Some(&ctx.user_id.0))?;
    let variables_schema = req
        .variables_schema
        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
    // task 2.4：body 只能引用 variables_schema 允许的变量。
    validate_template_variables(&req.body, &variables_schema)?;

    let now = TimestampMicros::now();
    let user_id = if req.scope == "user" {
        Some(ctx.user_id.0.clone())
    } else {
        None
    };
    let id = Id::new();
    let t = AgentPromptTemplate {
        id: id.clone(),
        org_id: Some(ctx.org_id.0.clone()),
        user_id,
        scope: req.scope.clone(),
        builtin_key: req.builtin_key.clone(),
        purpose: req.purpose.clone(),
        name: req.name.clone(),
        body: req.body.clone(),
        variables_schema,
        is_default: false,
        enabled: req.enabled.unwrap_or(true),
        version: 1,
        parent_id: req.parent_id.clone(),
        created_by: Some(ctx.user_id.0.clone()),
        updated_by: Some(ctx.user_id.0.clone()),
        created_at: now,
        updated_at: now,
    };
    let saved = state.intelligence.prompts.create(t).await?;
    activity_audit::record(
        &state,
        &ctx,
        "intelligence.prompt.created",
        "intelligence_prompt",
        &saved.id.0,
        json!({
            "scope": saved.scope,
            "purpose": saved.purpose,
            "builtin_key": saved.builtin_key,
            "version": saved.version,
            "prompt_hash": prompt_hash(&saved.body),
        }),
    )
    .await;
    Ok(Json(to_resp(saved)))
}

async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<PromptResp>> {
    require_license(&state)?;
    if req.name.trim().is_empty() || req.body.trim().is_empty() {
        return Err(Error::invalid("name and body are required"));
    }
    let existing = state.intelligence.prompts.get(&Id(id.clone())).await?;
    authorize_scope(&ctx, &existing.scope, existing.user_id.as_deref())?;
    // org override 也要校验本 org。
    if existing.org_id.as_deref() != Some(ctx.org_id.0.as_str()) {
        return Err(Error::forbidden("prompt belongs to another org"));
    }
    let variables_schema = req
        .variables_schema
        .unwrap_or(existing.variables_schema.clone());
    validate_template_variables(&req.body, &variables_schema)?;

    let mut next = existing.clone();
    next.name = req.name.clone();
    next.body = req.body.clone();
    next.variables_schema = variables_schema;
    next.enabled = req.enabled.unwrap_or(existing.enabled);
    next.updated_by = Some(ctx.user_id.0.clone());
    let saved = state.intelligence.prompts.update(next).await?;
    activity_audit::record(
        &state,
        &ctx,
        "intelligence.prompt.updated",
        "intelligence_prompt",
        &saved.id.0,
        json!({
            "scope": saved.scope,
            "purpose": saved.purpose,
            "version": saved.version,
            "prompt_hash": prompt_hash(&saved.body),
        }),
    )
    .await;
    Ok(Json(to_resp(saved)))
}

async fn set_default(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<PromptResp>> {
    require_license(&state)?;
    let existing = state.intelligence.prompts.get(&Id(id.clone())).await?;
    authorize_scope(&ctx, &existing.scope, existing.user_id.as_deref())?;
    if existing.org_id.as_deref() != Some(ctx.org_id.0.as_str()) {
        return Err(Error::forbidden("prompt belongs to another org"));
    }
    state
        .intelligence
        .prompts
        .set_default(&Id(id.clone()))
        .await?;
    activity_audit::record(
        &state,
        &ctx,
        "intelligence.prompt.default_set",
        "intelligence_prompt",
        &id,
        json!({ "scope": existing.scope, "purpose": existing.purpose }),
    )
    .await;
    let saved = state.intelligence.prompts.get(&Id(id)).await?;
    Ok(Json(to_resp(saved)))
}

async fn restore(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<PromptResp>> {
    require_license(&state)?;
    let existing = state.intelligence.prompts.get(&Id(id.clone())).await?;
    authorize_scope(&ctx, &existing.scope, existing.user_id.as_deref())?;
    if existing.org_id.as_deref() != Some(ctx.org_id.0.as_str()) {
        return Err(Error::forbidden("prompt belongs to another org"));
    }
    let builtin_key = existing
        .builtin_key
        .clone()
        .ok_or_else(|| Error::invalid("override has no builtin parent to restore from"))?;
    let builtin = state.intelligence.prompts.get_builtin(&builtin_key).await?;
    // 用 builtin body/schema 覆盖 override，递增 version。
    let mut next = existing.clone();
    next.body = builtin.body;
    next.variables_schema = builtin.variables_schema;
    next.updated_by = Some(ctx.user_id.0.clone());
    let saved = state.intelligence.prompts.update(next).await?;
    activity_audit::record(
        &state,
        &ctx,
        "intelligence.prompt.restored",
        "intelligence_prompt",
        &saved.id.0,
        json!({
            "scope": saved.scope,
            "purpose": saved.purpose,
            "version": saved.version,
            "restored_from_builtin": builtin_key,
            "prompt_hash": prompt_hash(&saved.body),
        }),
    )
    .await;
    Ok(Json(to_resp(saved)))
}

async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    require_license(&state)?;
    let existing = state.intelligence.prompts.get(&Id(id.clone())).await?;
    authorize_scope(&ctx, &existing.scope, existing.user_id.as_deref())?;
    if existing.org_id.as_deref() != Some(ctx.org_id.0.as_str()) {
        return Err(Error::forbidden("prompt belongs to another org"));
    }
    state.intelligence.prompts.delete(&Id(id.clone())).await?;
    activity_audit::record(
        &state,
        &ctx,
        "intelligence.prompt.deleted",
        "intelligence_prompt",
        &id,
        json!({ "scope": existing.scope, "purpose": existing.purpose }),
    )
    .await;
    Ok(Json(json!({ "deleted": true })))
}
