// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Permission policies: the built-in ones to read, and an organization's own to manage. A
//! policy is a JSON document after the AWS IAM policy language; it is resolved when it is saved,
//! against the permissions of the session it is saved in: the organization permissions of the
//! catalog, or the platform permissions in the platform organization.

use std::collections::BTreeSet;

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::iam::grant_limit;
use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::{IamScope, catalog::IamPermissionScope, permission, policy::PolicyDocument},
    infra::persistence::repositories::iam::policies::{IamPolicy, IamPolicyWithUsage},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/iam/policies", get(list).post(create))
        .route("/iam/policies/{id}", get(read).patch(update).delete(delete))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PolicyResp {
    id: String,
    key: String,
    name: String,
    description: String,
    builtin: bool,
    document: Value,
    permissions: Vec<String>,
    usage: PolicyUsageResp,
    created_by: Option<String>,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, Serialize)]
struct PolicyUsageResp {
    roles: i64,
    tokens: i64,
    total: i64,
}

#[derive(Debug, Deserialize)]
struct CreateReq {
    key: String,
    name: String,
    #[serde(default)]
    description: String,
    document: Value,
}

#[derive(Debug, Deserialize)]
struct UpdateReq {
    name: String,
    #[serde(default)]
    description: String,
    document: Value,
}

fn to_resp(entry: IamPolicyWithUsage) -> PolicyResp {
    let IamPolicyWithUsage { policy, usage } = entry;

    PolicyResp {
        builtin: policy.builtin(),
        id: policy.id.0,
        key: policy.key,
        name: policy.name,
        description: policy.description,
        document: policy.document,
        permissions: policy.permissions,
        usage: PolicyUsageResp {
            roles: usage.roles,
            tokens: usage.tokens,
            total: usage.total(),
        },
        created_by: policy.created_by.map(|id| id.0),
        created_at: policy.created_at.0,
        updated_at: policy.updated_at.0,
    }
}

#[permission(any("iam.policies.read", "sys.administrators.read"))]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<PolicyResp>>> {
    let mut policies = state.iam.policies.list(&ctx.org_id).await?;

    // The built-in policies are about an organization's work, so they mean nothing on the platform.
    if ctx.scope == IamScope::System {
        policies.retain(|entry| !entry.policy.builtin());
    }

    Ok(Json(policies.into_iter().map(to_resp).collect()))
}

#[permission(any("iam.policies.read", "sys.administrators.read"))]
async fn read(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<PolicyResp>> {
    let policy = state.iam.policies.get(&ctx.org_id, &Id(id)).await?;
    Ok(Json(to_resp(policy)))
}

#[permission(any("iam.policies.manage", "sys.administrators.manage"))]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateReq>,
) -> Result<Json<PolicyResp>> {
    let key = normalize_key(&req.key)?;
    let name = normalize_name(&req.name)?;
    let (document, permissions) = resolve_document(&state, &ctx, &req.document).await?;
    grant_limit::require_held(&ctx, &permissions)?;
    let now = TimestampMicros::now();

    let saved = state
        .iam
        .policies
        .create(IamPolicy {
            id: Id::new(),
            org_id: Some(ctx.org_id.clone()),
            key,
            name,
            description: req.description.trim().to_string(),
            document,
            permissions,
            created_by: Some(ctx.user_id.clone()),
            created_at: now,
            updated_at: now,
        })
        .await?;

    let policy = state.iam.policies.get(&ctx.org_id, &saved.id).await?;
    Ok(Json(to_resp(policy)))
}

#[permission(any("iam.policies.manage", "sys.administrators.manage"))]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<PolicyResp>> {
    let existing = state.iam.policies.get(&ctx.org_id, &Id(id)).await?.policy;

    if existing.builtin() {
        return Err(Error::forbidden(
            "built-in permission policies cannot be edited",
        ));
    }

    let (document, permissions) = resolve_document(&state, &ctx, &req.document).await?;

    // Roles that follow the policy get whatever it gains.
    grant_limit::require_held(
        &ctx,
        permissions
            .iter()
            .filter(|permission| !existing.permissions.contains(permission)),
    )?;

    let saved = state
        .iam
        .policies
        .update(IamPolicy {
            name: normalize_name(&req.name)?,
            description: req.description.trim().to_string(),
            document,
            permissions,
            updated_at: TimestampMicros::now(),
            ..existing
        })
        .await?;

    let policy = state.iam.policies.get(&ctx.org_id, &saved.id).await?;
    Ok(Json(to_resp(policy)))
}

#[permission(any("iam.policies.delete", "sys.administrators.manage"))]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let id = Id(id);
    let existing = state.iam.policies.get(&ctx.org_id, &id).await?.policy;

    if existing.builtin() {
        return Err(Error::forbidden(
            "built-in permission policies cannot be deleted",
        ));
    }

    state.iam.policies.delete(&ctx.org_id, &id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// The document as it is stored, and the permissions it allows among those of the session.
async fn resolve_document(
    state: &AppState,
    ctx: &IamContext,
    document: &Value,
) -> Result<(Value, Vec<String>)> {
    let parsed = PolicyDocument::parse(document)?;
    let catalog = state.iam.access.repository().permission_catalog().await?;

    let session_scope = match ctx.scope {
        IamScope::System => IamPermissionScope::Platform,
        IamScope::Organization | IamScope::ApiToken => IamPermissionScope::Organization,
    };

    let allowed = catalog
        .permissions
        .into_iter()
        .filter(|permission| permission.scope == session_scope)
        .map(|permission| permission.key)
        .collect::<BTreeSet<_>>();

    let permissions = parsed.resolve(&allowed)?;

    let stored = serde_json::to_value(&parsed)
        .map_err(|error| Error::internal(format!("serialize policy document: {error}")))?;

    Ok((stored, permissions))
}

fn normalize_key(input: &str) -> Result<String> {
    let key = input.trim().to_lowercase();

    if !(2..=64).contains(&key.len()) {
        return Err(Error::invalid("policy key must be 2..64 characters"));
    }

    let mut chars = key.chars();

    if !chars.next().is_some_and(|first| first.is_ascii_lowercase()) {
        return Err(Error::invalid("policy key must start with a letter"));
    }

    if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
        return Err(Error::invalid(
            "policy key can only contain lowercase letters, numbers, and underscore",
        ));
    }

    Ok(key)
}

fn normalize_name(input: &str) -> Result<String> {
    let name = input.trim();

    if name.is_empty() {
        return Err(Error::invalid("policy name must not be empty"));
    }

    if name.chars().count() > 128 {
        return Err(Error::invalid("policy name must be <= 128 characters"));
    }

    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_a_lowercase_word() {
        assert_eq!(
            normalize_key(" Read_Only ").expect("valid key"),
            "read_only"
        );
        assert!(normalize_key("1st").is_err());
        assert!(normalize_key("read-only").is_err());
        assert!(normalize_key("x").is_err());
    }

    #[test]
    fn a_name_is_trimmed_and_bounded() {
        assert_eq!(normalize_name("  Readers ").expect("valid name"), "Readers");
        assert!(normalize_name("  ").is_err());
        assert!(normalize_name(&"名".repeat(129)).is_err());
    }
}
