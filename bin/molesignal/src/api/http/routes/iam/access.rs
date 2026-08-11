// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Unified IAM capability, binding, relationship, and sharing APIs.

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    api::{AppState, http::middleware::Permission},
    app::iam::{IamAccessRequest, IamCapabilitySnapshot, IamContext, validate_iam_conditions},
    domain::iam::{
        access::{
            IamCrossOrgGrant, IamCrossOrgGrantStatus, IamPrincipalType, IamResourceRelationship,
            IamRoleBinding,
        },
        catalog::{IamPermissionCatalog, IamPermissionScope},
        permission,
    },
    infra::persistence::repositories::audit_events::AuditEvent,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const RESOURCE_TYPES: &[&str] = &[
    "alert",
    "dashboard",
    "folder",
    "function",
    "pipeline",
    "report",
    "saved_view",
    "schedule",
    "stream",
];

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/iam/permissions", get(permission_catalog))
        .route("/iam/capabilities", get(capabilities))
        .route("/iam/share-targets", get(list_share_targets))
        .route(
            "/iam/role-bindings",
            get(list_role_bindings).post(create_role_binding),
        )
        .route("/iam/role-bindings/{id}", delete(delete_role_binding))
        .route(
            "/iam/relationships",
            get(list_relationships).post(create_relationship),
        )
        .route("/iam/relationships/{id}", delete(delete_relationship))
        .route(
            "/iam/cross-org-grants",
            get(list_cross_org_grants).post(create_cross_org_grant),
        )
        .route(
            "/iam/cross-org-grants/{id}/accept",
            post(accept_cross_org_grant),
        )
        .route(
            "/iam/cross-org-grants/{id}/revoke",
            post(revoke_cross_org_grant),
        )
        .route("/iam/evaluate-batch", post(evaluate_batch))
}

#[permission(any("iam.roles.read", "iam.policies.read"))]
async fn permission_catalog(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<IamPermissionCatalog>> {
    Ok(Json(
        state.iam.access.repository().permission_catalog().await?,
    ))
}

async fn capabilities(
    Extension(snapshot): Extension<IamCapabilitySnapshot>,
) -> Result<Json<IamCapabilitySnapshot>> {
    Ok(Json(snapshot))
}

#[derive(Debug, Serialize)]
struct IamShareTarget {
    id: String,
    name: String,
}

#[permission("iam.policies.manage")]
async fn list_share_targets(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<Vec<IamShareTarget>>> {
    let mut targets = state
        .iam
        .service
        .orgs
        .list()
        .await?
        .into_iter()
        .filter(|organization| !organization.system && organization.id != context.org_id)
        .map(|organization| IamShareTarget {
            id: organization.id.0,
            name: organization.name,
        })
        .collect::<Vec<_>>();
    targets.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(Json(targets))
}

#[derive(Debug, Deserialize)]
struct CreateRoleBindingRequest {
    role_id: String,
    principal_type: IamPrincipalType,
    principal_id: String,
    #[serde(default)]
    resource_type: Option<String>,
    #[serde(default)]
    resource_id: Option<String>,
    #[serde(default = "empty_object")]
    conditions: Value,
    #[serde(default)]
    starts_at_micros: Option<i64>,
    #[serde(default)]
    expires_at_micros: Option<i64>,
}

#[derive(Debug, Serialize)]
struct MutationResponse<T> {
    value: T,
    version: u64,
}

#[derive(Debug, Serialize)]
struct DeleteResponse {
    deleted: bool,
    version: u64,
}

#[permission("iam.policies.read")]
async fn list_role_bindings(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<Vec<IamRoleBinding>>> {
    Ok(Json(
        state
            .iam
            .access
            .repository()
            .list_role_bindings(&context.org_id)
            .await?,
    ))
}

#[permission("iam.policies.manage")]
async fn create_role_binding(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Json(request): Json<CreateRoleBindingRequest>,
) -> Result<Json<MutationResponse<IamRoleBinding>>> {
    validate_window(request.starts_at_micros, request.expires_at_micros)?;
    validate_iam_conditions(&request.conditions)?;
    validate_resource_selector(
        request.resource_type.as_deref(),
        request.resource_id.as_deref(),
    )?;
    state
        .iam
        .roles
        .get(&context.org_id, &Id::from_string(&request.role_id))
        .await?;
    validate_principal(
        &state,
        &context.org_id,
        request.principal_type,
        &request.principal_id,
    )
    .await?;
    let binding = IamRoleBinding {
        id: Id::new(),
        organization_id: context.org_id.clone(),
        role_id: Id::from_string(request.role_id),
        principal_type: request.principal_type,
        principal_id: Id::from_string(request.principal_id),
        resource_type: request.resource_type,
        resource_id: request.resource_id,
        conditions: request.conditions,
        starts_at: request.starts_at_micros.map(TimestampMicros),
        expires_at: request.expires_at_micros.map(TimestampMicros),
        created_by: context.user_id.clone(),
        created_at: TimestampMicros::now(),
    };
    let (binding, version) = state
        .iam
        .access
        .repository()
        .create_role_binding(binding)
        .await?;
    record_mutation(
        &state,
        &context,
        "iam.role_binding.create",
        "iam_role_binding",
        &binding.id,
        serde_json::json!({"role_id": binding.role_id.0}),
    )
    .await?;
    Ok(Json(MutationResponse {
        value: binding,
        version,
    }))
}

#[permission("iam.policies.manage")]
async fn delete_role_binding(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<DeleteResponse>> {
    let id = Id::from_string(id);
    let version = state
        .iam
        .access
        .repository()
        .delete_role_binding(&context.org_id, &id)
        .await?;
    record_mutation(
        &state,
        &context,
        "iam.role_binding.delete",
        "iam_role_binding",
        &id,
        Value::Null,
    )
    .await?;
    Ok(Json(DeleteResponse {
        deleted: true,
        version,
    }))
}

#[derive(Debug, Deserialize)]
struct CreateRelationshipRequest {
    resource_type: String,
    resource_id: String,
    role_id: String,
    subject_type: IamPrincipalType,
    subject_id: String,
    #[serde(default)]
    container_type: Option<String>,
    #[serde(default)]
    container_id: Option<String>,
}

#[permission("iam.policies.read")]
async fn list_relationships(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<Vec<IamResourceRelationship>>> {
    Ok(Json(
        state
            .iam
            .access
            .repository()
            .list_relationships(&context.org_id)
            .await?,
    ))
}

#[permission("iam.policies.manage")]
async fn create_relationship(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Json(request): Json<CreateRelationshipRequest>,
) -> Result<Json<MutationResponse<IamResourceRelationship>>> {
    validate_resource_type(&request.resource_type)?;
    validate_nonempty_id("resource_id", &request.resource_id)?;
    validate_resource_selector(
        request.container_type.as_deref(),
        request.container_id.as_deref(),
    )?;
    validate_nonempty_id("role_id", &request.role_id)?;
    let role_id = Id::from_string(request.role_id);
    state.iam.roles.get(&context.org_id, &role_id).await?;
    validate_principal(
        &state,
        &context.org_id,
        request.subject_type,
        &request.subject_id,
    )
    .await?;
    let relationship = IamResourceRelationship {
        id: Id::new(),
        organization_id: context.org_id.clone(),
        resource_type: request.resource_type,
        resource_id: request.resource_id,
        role_id,
        subject_type: request.subject_type,
        subject_id: Id::from_string(request.subject_id),
        container_type: request.container_type,
        container_id: request.container_id,
        created_by: context.user_id.clone(),
        created_at: TimestampMicros::now(),
    };
    let (relationship, version) = state
        .iam
        .access
        .repository()
        .create_relationship(relationship)
        .await?;
    record_mutation(
        &state,
        &context,
        "iam.relationship.create",
        "iam_relationship",
        &relationship.id,
        serde_json::json!({"role_id": relationship.role_id}),
    )
    .await?;
    Ok(Json(MutationResponse {
        value: relationship,
        version,
    }))
}

#[permission("iam.policies.manage")]
async fn delete_relationship(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<DeleteResponse>> {
    let id = Id::from_string(id);
    let version = state
        .iam
        .access
        .repository()
        .delete_relationship(&context.org_id, &id)
        .await?;
    record_mutation(
        &state,
        &context,
        "iam.relationship.delete",
        "iam_relationship",
        &id,
        Value::Null,
    )
    .await?;
    Ok(Json(DeleteResponse {
        deleted: true,
        version,
    }))
}

#[derive(Debug, Deserialize)]
struct CreateCrossOrgGrantRequest {
    target_organization_id: String,
    grantee_type: IamPrincipalType,
    grantee_id: String,
    resource_type: String,
    resource_selector: Value,
    permissions: Vec<String>,
    #[serde(default = "empty_object")]
    conditions: Value,
    #[serde(default)]
    starts_at_micros: Option<i64>,
    #[serde(default)]
    expires_at_micros: Option<i64>,
}

#[permission("iam.policies.read")]
async fn list_cross_org_grants(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<Vec<IamCrossOrgGrant>>> {
    Ok(Json(
        state
            .iam
            .access
            .repository()
            .list_cross_org_grants(&context.org_id)
            .await?,
    ))
}

#[permission("iam.policies.manage")]
async fn create_cross_org_grant(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Json(request): Json<CreateCrossOrgGrantRequest>,
) -> Result<Json<MutationResponse<IamCrossOrgGrant>>> {
    let target_org = Id::from_string(request.target_organization_id);
    if target_org == context.org_id {
        return Err(Error::invalid(
            "cross-organization grant target must be another organization",
        ));
    }
    state.iam.service.orgs.get(&target_org).await?;
    validate_resource_type(&request.resource_type)?;
    validate_window(request.starts_at_micros, request.expires_at_micros)?;
    validate_iam_conditions(&request.conditions)?;
    let resource_ids = validate_cross_org_selector(&request.resource_selector)?;
    let permissions = state
        .iam
        .access
        .repository()
        .validate_permission_keys(&request.permissions, IamPermissionScope::Organization)
        .await?;
    if permissions.is_empty() {
        return Err(Error::invalid(
            "cross-organization grants require at least one registered permission",
        ));
    }
    validate_principal(
        &state,
        &target_org,
        request.grantee_type,
        &request.grantee_id,
    )
    .await?;

    for permission in &permissions {
        if resource_ids.is_empty() {
            Permission::require_key(&context, permission)?;
        } else {
            for resource_id in &resource_ids {
                let decision = state
                    .iam
                    .access
                    .evaluate(
                        &context,
                        &IamAccessRequest {
                            permission: permission.clone(),
                            target: Some(crate::app::iam::IamTarget {
                                organization_id: None,
                                resource_type: request.resource_type.clone(),
                                resource_id: resource_id.clone(),
                                container_type: None,
                                container_id: None,
                            }),
                            attributes: Default::default(),
                        },
                    )
                    .await?;
                if !decision.allowed {
                    return Err(Error::forbidden(format!(
                        "cannot delegate {permission} for resource {resource_id}"
                    )));
                }
            }
        }
    }

    let now = TimestampMicros::now();
    let grant = IamCrossOrgGrant {
        id: Id::new(),
        source_organization_id: context.org_id.clone(),
        target_organization_id: target_org,
        grantee_type: request.grantee_type,
        grantee_id: Id::from_string(request.grantee_id),
        resource_type: request.resource_type,
        resource_selector: request.resource_selector,
        permissions,
        conditions: request.conditions,
        starts_at: request.starts_at_micros.map(TimestampMicros),
        expires_at: request.expires_at_micros.map(TimestampMicros),
        status: IamCrossOrgGrantStatus::Pending,
        approved_by: None,
        approved_at: None,
        revoked_by: None,
        revoked_at: None,
        created_by: context.user_id.clone(),
        created_at: now,
    };
    let (grant, version) = state
        .iam
        .access
        .repository()
        .create_cross_org_grant(grant)
        .await?;
    record_mutation(
        &state,
        &context,
        "iam.cross_org_grant.create",
        "cross_org_grant",
        &grant.id,
        serde_json::json!({"permissions": grant.permissions}),
    )
    .await?;
    Ok(Json(MutationResponse {
        value: grant,
        version,
    }))
}

#[permission("iam.policies.manage")]
async fn accept_cross_org_grant(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<MutationResponse<IamCrossOrgGrant>>> {
    change_grant_status(
        &state,
        &context,
        Id::from_string(id),
        IamCrossOrgGrantStatus::Active,
    )
    .await
}

#[permission("iam.policies.manage")]
async fn revoke_cross_org_grant(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<MutationResponse<IamCrossOrgGrant>>> {
    change_grant_status(
        &state,
        &context,
        Id::from_string(id),
        IamCrossOrgGrantStatus::Revoked,
    )
    .await
}

async fn change_grant_status(
    state: &AppState,
    context: &IamContext,
    id: Id,
    status: IamCrossOrgGrantStatus,
) -> Result<Json<MutationResponse<IamCrossOrgGrant>>> {
    let (grant, version) = state
        .iam
        .access
        .repository()
        .set_cross_org_grant_status(
            &context.org_id,
            &id,
            status,
            &context.user_id,
            TimestampMicros::now(),
        )
        .await?;
    record_mutation(
        state,
        context,
        match status {
            IamCrossOrgGrantStatus::Active => "iam.cross_org_grant.accept",
            IamCrossOrgGrantStatus::Revoked => "iam.cross_org_grant.revoke",
            IamCrossOrgGrantStatus::Pending => "iam.cross_org_grant.update",
        },
        "cross_org_grant",
        &id,
        serde_json::json!({"status": status}),
    )
    .await?;
    Ok(Json(MutationResponse {
        value: grant,
        version,
    }))
}

#[derive(Debug, Deserialize)]
struct EvaluateBatchRequest {
    requests: Vec<IamAccessRequest>,
}

#[derive(Debug, Serialize)]
struct EvaluateBatchResponse {
    decisions: Vec<crate::app::iam::IamDecision>,
}

async fn evaluate_batch(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Json(request): Json<EvaluateBatchRequest>,
) -> Result<Json<EvaluateBatchResponse>> {
    Ok(Json(EvaluateBatchResponse {
        decisions: state
            .iam
            .access
            .evaluate_batch(&context, &request.requests)
            .await?,
    }))
}

async fn validate_principal(
    state: &AppState,
    organization_id: &Id,
    principal_type: IamPrincipalType,
    principal_id: &str,
) -> Result<()> {
    validate_nonempty_id("principal_id", principal_id)?;
    match principal_type {
        IamPrincipalType::User => {
            let found = state
                .iam
                .service
                .iam_memberships
                .list_for_org(organization_id)
                .await?
                .into_iter()
                .any(|membership| membership.user_id.0 == principal_id);
            if !found {
                return Err(Error::invalid(
                    "principal user must be a member of the selected organization",
                ));
            }
        }
        IamPrincipalType::Team => {
            let team = state.iam.teams.get(&Id::from_string(principal_id)).await?;
            if team.org_id != *organization_id {
                return Err(Error::invalid(
                    "principal team must belong to the selected organization",
                ));
            }
        }
        IamPrincipalType::Organization if principal_id == organization_id.0 => {}
        IamPrincipalType::Organization => {
            return Err(Error::invalid(
                "organization principal must match the selected organization",
            ));
        }
        IamPrincipalType::Group | IamPrincipalType::ServiceAccount => {
            return Err(Error::invalid(
                "group and service-account principals are not available yet",
            ));
        }
    }
    Ok(())
}

fn validate_resource_selector(
    resource_type: Option<&str>,
    resource_id: Option<&str>,
) -> Result<()> {
    match (resource_type, resource_id) {
        (None, None) => Ok(()),
        (Some(resource_type), resource_id) => {
            validate_resource_type(resource_type)?;
            if let Some(resource_id) = resource_id {
                validate_nonempty_id("resource_id", resource_id)?;
            }
            Ok(())
        }
        (None, Some(_)) => Err(Error::invalid(
            "resource_id requires a registered resource_type",
        )),
    }
}

fn validate_resource_type(resource_type: &str) -> Result<()> {
    if RESOURCE_TYPES.contains(&resource_type) {
        Ok(())
    } else {
        Err(Error::invalid(format!(
            "unsupported IAM resource type: {resource_type}"
        )))
    }
}

fn validate_nonempty_id(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 255 {
        Err(Error::invalid(format!(
            "{label} must contain 1..255 characters"
        )))
    } else {
        Ok(())
    }
}

fn validate_window(starts_at: Option<i64>, expires_at: Option<i64>) -> Result<()> {
    if starts_at.is_some_and(|value| value < 0)
        || expires_at.is_some_and(|value| value < 0)
        || matches!((starts_at, expires_at), (Some(start), Some(end)) if start >= end)
    {
        Err(Error::invalid("invalid IAM validity window"))
    } else {
        Ok(())
    }
}

fn validate_cross_org_selector(selector: &Value) -> Result<Vec<String>> {
    let Some(object) = selector.as_object() else {
        return Err(Error::invalid("resource_selector must be an object"));
    };
    if object.len() != 1 {
        return Err(Error::invalid(
            "resource_selector must contain exactly one of ids or all",
        ));
    }
    if object.get("all").and_then(Value::as_bool) == Some(true) {
        return Ok(Vec::new());
    }
    let Some(ids) = object.get("ids").and_then(Value::as_array) else {
        return Err(Error::invalid(
            "resource_selector must contain ids or all=true",
        ));
    };
    if ids.is_empty() || ids.len() > 100 {
        return Err(Error::invalid(
            "resource_selector ids must contain 1..100 values",
        ));
    }
    ids.iter()
        .map(|value| {
            let value = value
                .as_str()
                .ok_or_else(|| Error::invalid("resource_selector ids must be strings"))?;
            validate_nonempty_id("resource_selector id", value)?;
            Ok(value.to_string())
        })
        .collect()
}

async fn record_mutation(
    state: &AppState,
    context: &IamContext,
    action: &str,
    target_kind: &str,
    target_id: &Id,
    payload: Value,
) -> Result<()> {
    state
        .iam
        .audit_events
        .record(AuditEvent {
            id: Id::new(),
            org_id: context.org_id.clone(),
            actor_kind: "user".into(),
            actor_id: context.user_id.0.clone(),
            action: action.into(),
            target_kind: Some(target_kind.into()),
            target_id: Some(target_id.0.clone()),
            ip: None,
            user_agent: None,
            payload,
            ts: TimestampMicros::now(),
        })
        .await
}

fn empty_object() -> Value {
    serde_json::json!({})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_org_selector_is_bounded() {
        assert_eq!(
            validate_cross_org_selector(&serde_json::json!({"ids": ["d1", "d2"]})).unwrap(),
            vec!["d1", "d2"]
        );
        assert!(validate_cross_org_selector(&serde_json::json!({"ids": [], "all": true})).is_err());
        assert!(validate_cross_org_selector(&serde_json::json!({"all": true})).is_ok());
    }
}
