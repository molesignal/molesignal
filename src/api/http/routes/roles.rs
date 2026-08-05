// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! IAM role catalog CRUD.

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    infra::persistence::repositories::iam::roles::{IamRole, RoleUsage},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/roles", get(list).post(create))
        .route("/roles/{id}", axum::routing::patch(update).delete(delete))
}

#[derive(Debug, Serialize)]
struct RoleResp {
    id: String,
    key: String,
    name: String,
    description: String,
    builtin: bool,
    role_type: String,
    scope: String,
    permissions: Vec<String>,
    usage: RoleUsageResp,
    created_at_micros: i64,
    updated_at_micros: i64,
}

#[derive(Debug, Serialize)]
struct RoleUsageResp {
    memberships: i64,
    api_tokens: i64,
    invitations: i64,
    bindings: i64,
    total: i64,
}

#[derive(Debug, Deserialize)]
struct CreateReq {
    key: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    permissions: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateReq {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    permissions: Vec<String>,
}

fn usage_resp(usage: RoleUsage) -> RoleUsageResp {
    RoleUsageResp {
        memberships: usage.memberships,
        api_tokens: usage.api_tokens,
        invitations: usage.invitations,
        bindings: usage.bindings,
        total: usage.total(),
    }
}

async fn to_resp(state: &AppState, org_id: &Id, role: IamRole) -> Result<RoleResp> {
    let usage = state.iam.roles.usage_by_key(org_id, &role.key).await?;
    Ok(RoleResp {
        id: role.id.0,
        key: role.key,
        name: role.name,
        description: role.description,
        builtin: role.builtin,
        role_type: role.role_type,
        scope: role.scope,
        permissions: role.permissions,
        usage: usage_resp(usage),
        created_at_micros: role.created_at.0,
        updated_at_micros: role.updated_at.0,
    })
}

#[permission("iam.roles.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<RoleResp>>> {
    state.iam.roles.ensure_builtin_roles(&ctx.org_id).await?;
    let roles = state.iam.roles.list(&ctx.org_id).await?;
    let mut out = Vec::with_capacity(roles.len());
    for role in roles {
        out.push(to_resp(&state, &ctx.org_id, role).await?);
    }
    Ok(Json(out))
}

#[permission("iam.roles.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateReq>,
) -> Result<Json<RoleResp>> {
    state.iam.roles.ensure_builtin_roles(&ctx.org_id).await?;
    let key = normalize_key(&req.key)?;
    if state
        .iam
        .roles
        .list(&ctx.org_id)
        .await?
        .iter()
        .any(|role| role.key == key)
    {
        return Err(Error::conflict("role key already exists"));
    }
    let name = normalize_name(&req.name)?;
    let permissions = normalize_permissions(req.permissions);
    let now = TimestampMicros::now();
    let role = IamRole {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        key,
        name,
        description: req.description.trim().to_string(),
        builtin: false,
        role_type: "organization".into(),
        scope: "organization".into(),
        permissions,
        created_at: now,
        updated_at: now,
    };
    let saved = state.iam.roles.create(role).await?;
    Ok(Json(to_resp(&state, &ctx.org_id, saved).await?))
}

#[permission("iam.roles.manage")]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<RoleResp>> {
    state.iam.roles.ensure_builtin_roles(&ctx.org_id).await?;
    let existing = state.iam.roles.get(&ctx.org_id, &Id(id)).await?;
    if existing.builtin {
        return Err(Error::forbidden("built-in roles cannot be edited"));
    }
    let role = IamRole {
        id: existing.id,
        org_id: existing.org_id,
        key: existing.key,
        name: normalize_name(&req.name)?,
        description: req.description.trim().to_string(),
        builtin: false,
        role_type: existing.role_type,
        scope: existing.scope,
        permissions: normalize_permissions(req.permissions),
        created_at: existing.created_at,
        updated_at: TimestampMicros::now(),
    };
    let saved = state.iam.roles.update(role).await?;
    Ok(Json(to_resp(&state, &ctx.org_id, saved).await?))
}

#[permission("iam.roles.manage")]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    state.iam.roles.ensure_builtin_roles(&ctx.org_id).await?;
    let id = Id(id);
    let role = state.iam.roles.get(&ctx.org_id, &id).await?;
    if role.builtin {
        return Err(Error::forbidden("built-in roles cannot be deleted"));
    }
    if state
        .iam
        .sso_providers
        .list(&ctx.org_id)
        .await?
        .iter()
        .any(|provider| provider.config.references_role(&id))
    {
        return Err(Error::conflict(
            "role is referenced by an SSO provider mapping",
        ));
    }
    let usage = state.iam.roles.usage_by_key(&ctx.org_id, &role.key).await?;
    if usage.total() > 0 {
        return Err(Error::conflict(format!(
            "role is in use: memberships={}, api_tokens={}, invitations={}, bindings={}",
            usage.memberships, usage.api_tokens, usage.invitations, usage.bindings
        )));
    }
    state.iam.roles.delete(&ctx.org_id, &id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

fn normalize_key(input: &str) -> Result<String> {
    let key = input.trim().to_lowercase();
    if !(2..=64).contains(&key.len()) {
        return Err(Error::invalid("role key must be 2..64 characters"));
    }
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return Err(Error::invalid("role key must not be empty"));
    };
    if !first.is_ascii_lowercase() {
        return Err(Error::invalid("role key must start with a letter"));
    }
    if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
        return Err(Error::invalid(
            "role key can only contain lowercase letters, numbers, and underscore",
        ));
    }
    Ok(key)
}

fn normalize_name(input: &str) -> Result<String> {
    let name = input.trim();
    if name.is_empty() {
        return Err(Error::invalid("role name must not be empty"));
    }
    if name.len() > 128 {
        return Err(Error::invalid("role name must be <= 128 characters"));
    }
    Ok(name.to_string())
}

fn normalize_permissions(input: Vec<String>) -> Vec<String> {
    let mut permissions = input
        .into_iter()
        .map(|permission| permission.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    permissions.sort();
    permissions.dedup();
    permissions
}
