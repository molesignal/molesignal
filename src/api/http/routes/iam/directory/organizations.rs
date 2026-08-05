// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Organization lifecycle endpoints.
//!
//! Tenant identity, platform-managed enabled state, deletion invariants, and
//! organization selection are kept together so suspension semantics cannot
//! drift across otherwise separate handlers.

use std::collections::HashSet;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch, post},
};
use serde::{Deserialize, Serialize};

use super::require_system_organization_management;
use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::{IamAssignedRole, IamScope, Organization, PLATFORM_ADMINISTRATOR_ROLE_PURPOSE},
    infra::persistence::repositories::audit_events::AuditEvent,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/orgs", get(list_orgs).post(create_org))
        .route("/orgs/{id}", patch(update_org).delete(delete_org))
        .route("/orgs/{id}/status", patch(set_org_status))
        .route("/orgs/{id}/select", post(select_org))
}

#[derive(Serialize)]
struct OrgView {
    id: String,
    name: String,
    slug: String,
    display_role: Option<String>,
    roles: Vec<IamAssignedRole>,
    system: bool,
    disabled: bool,
}

impl OrgView {
    fn from_org(organization: Organization, roles: Vec<IamAssignedRole>) -> Self {
        let display_role = roles
            .iter()
            .map(|role| role.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        Self {
            id: organization.id.0,
            name: organization.name,
            slug: organization.slug,
            display_role: (!display_role.is_empty()).then_some(display_role),
            roles,
            system: organization.system,
            disabled: organization.disabled,
        }
    }
}

async fn system_assigned_role(state: &AppState) -> Result<IamAssignedRole> {
    state
        .iam
        .access
        .repository()
        .role_for_purpose(
            &state.iam.system_org_id,
            PLATFORM_ADMINISTRATOR_ROLE_PURPOSE,
        )
        .await?
        .ok_or_else(|| {
            Error::internal("platform administrator IAM role is not materialized for `_sys`")
        })
}

async fn organization_root_role(state: &AppState, organization_id: &Id) -> Result<IamAssignedRole> {
    let role_id = state
        .iam
        .service
        .iam_memberships
        .role_id_for_purpose(organization_id, "organization_bootstrap")
        .await?;
    state
        .iam
        .access
        .repository()
        .role_summary(organization_id, &role_id)
        .await?
        .ok_or_else(|| Error::internal("organization root role was not materialized"))
}

async fn list_orgs(
    State(state): State<AppState>,
    axum::Extension(context): axum::Extension<IamContext>,
) -> Result<Json<Vec<OrgView>>> {
    let root = context.scope != IamScope::ApiToken
        && state
            .iam
            .platform_administrators
            .is_active(&context.user_id)
            .await?;
    if root {
        if context.scope == IamScope::System {
            require_system_organization_management(&state.iam.system_org_id, &context)?;
        }
        let mut views = Vec::new();
        for organization in state.iam.service.orgs.list().await? {
            let role = if organization.system {
                system_assigned_role(&state).await?
            } else {
                organization_root_role(&state, &organization.id).await?
            };
            views.push(OrgView::from_org(organization, vec![role]));
        }
        return Ok(Json(views));
    }

    if context.scope == IamScope::System {
        require_system_organization_management(&state.iam.system_org_id, &context)?;
        return Err(Error::forbidden("root system scope required"));
    }

    let memberships = state
        .iam
        .service
        .iam_memberships
        .list_for_user(&context.user_id)
        .await?;
    let mut organizations = Vec::new();
    let mut seen = HashSet::new();
    for membership in memberships {
        if !seen.insert(membership.org_id.clone()) {
            continue;
        }
        if let Ok(organization) = state.iam.service.orgs.get(&membership.org_id).await {
            let roles = state
                .iam
                .service
                .iam_memberships
                .assigned_roles(&context.user_id, &membership.org_id)
                .await?;
            organizations.push(OrgView::from_org(organization, roles));
        }
    }

    Ok(Json(organizations))
}

#[derive(Deserialize)]
struct CreateOrgReq {
    name: String,
    slug: String,
}

async fn create_org(
    State(state): State<AppState>,
    axum::Extension(context): axum::Extension<IamContext>,
    Json(request): Json<CreateOrgReq>,
) -> Result<(StatusCode, Json<OrgView>)> {
    require_system_organization_management(&state.iam.system_org_id, &context)?;
    let name = request.name.trim().to_string();
    let slug = request.slug.trim().to_string();
    if name.is_empty() {
        return Err(Error::invalid("name must not be empty"));
    }
    if slug.is_empty() {
        return Err(Error::invalid("slug must not be empty"));
    }

    let now = TimestampMicros::now();
    let organization = state
        .iam
        .service
        .orgs
        .create(Organization {
            id: Id::new(),
            name,
            slug,
            system: false,
            disabled: false,
            created_at: now,
        })
        .await?;
    let roles = vec![organization_root_role(&state, &organization.id).await?];
    Ok((
        StatusCode::CREATED,
        Json(OrgView::from_org(organization, roles)),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateOrgReq {
    name: Option<String>,
    /// Kept to return an explicit immutable-field error to legacy clients.
    slug: Option<String>,
}

async fn update_org(
    State(state): State<AppState>,
    axum::Extension(context): axum::Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<UpdateOrgReq>,
) -> Result<Json<OrgView>> {
    let target_org_id = Id::from_string(id);
    require_system_organization_management(&state.iam.system_org_id, &context)?;
    let current = state.iam.service.orgs.get(&target_org_id).await?;
    current.ensure_mutable()?;
    if request.slug.is_some() {
        return Err(Error::invalid(
            "organization slug is immutable after creation",
        ));
    }
    let organization = if let Some(name) = request.name {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(Error::invalid("name must not be empty"));
        }
        state
            .iam
            .service
            .orgs
            .update_name(&target_org_id, name)
            .await?
    } else {
        current
    };
    let roles = vec![organization_root_role(&state, &organization.id).await?];
    Ok(Json(OrgView::from_org(organization, roles)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetOrgStatusReq {
    disabled: bool,
}

async fn set_org_status(
    State(state): State<AppState>,
    axum::Extension(context): axum::Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<SetOrgStatusReq>,
) -> Result<Json<OrgView>> {
    require_system_organization_management(&state.iam.system_org_id, &context)?;
    let target_org_id = Id::from_string(id);
    let organization = state
        .iam
        .service
        .orgs
        .set_disabled(&target_org_id, request.disabled)
        .await?;
    let action = if request.disabled {
        "organization.disable"
    } else {
        "organization.enable"
    };
    let _ = state
        .iam
        .audit_events
        .record(AuditEvent {
            id: Id::new(),
            org_id: state.iam.system_org_id.clone(),
            actor_kind: "user".into(),
            actor_id: context.user_id.0,
            action: action.into(),
            target_kind: Some("organization".into()),
            target_id: Some(target_org_id.0),
            ip: None,
            user_agent: None,
            payload: serde_json::json!({ "disabled": request.disabled }),
            ts: TimestampMicros::now(),
        })
        .await;
    let roles = vec![organization_root_role(&state, &organization.id).await?];
    Ok(Json(OrgView::from_org(organization, roles)))
}

async fn delete_org(
    State(state): State<AppState>,
    axum::Extension(context): axum::Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let target_org_id = Id::from_string(id);
    require_system_organization_management(&state.iam.system_org_id, &context)?;
    let target = state.iam.service.orgs.get(&target_org_id).await?;
    target.ensure_mutable()?;
    if target_org_id == context.org_id {
        return Err(Error::invalid(
            "switch to another organization before deleting this one",
        ));
    }

    let tenants = state
        .iam
        .service
        .orgs
        .list()
        .await?
        .into_iter()
        .filter(|organization| !organization.system)
        .collect::<Vec<_>>();
    if tenants.len() <= 1 {
        return Err(Error::invalid("cannot delete the last organization"));
    }
    if !target.disabled
        && tenants
            .iter()
            .filter(|organization| !organization.disabled)
            .count()
            <= 1
    {
        return Err(Error::invalid(
            "cannot delete the last enabled tenant organization",
        ));
    }
    state.iam.service.orgs.delete(&target_org_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct SelectOrgResp {
    token: String,
    user_id: String,
    org_id: String,
    org_name: String,
    display_role: String,
    roles: Vec<IamAssignedRole>,
    system: bool,
}

async fn select_org(
    State(state): State<AppState>,
    axum::Extension(context): axum::Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<SelectOrgResp>> {
    let target_org_id = Id::from_string(id);
    let organization = state.iam.service.orgs.get(&target_org_id).await?;
    organization.ensure_enabled()?;
    let root = context.scope != IamScope::ApiToken
        && state
            .iam
            .platform_administrators
            .is_active(&context.user_id)
            .await?;
    if organization.system {
        if !root {
            return Err(Error::not_found("organization"));
        }
        return select_system_organization(state, context, target_org_id, organization).await;
    }

    let roles = if root {
        vec![organization_root_role(&state, &target_org_id).await?]
    } else {
        state
            .iam
            .service
            .iam_memberships
            .list_for_user(&context.user_id)
            .await?
            .into_iter()
            .find(|membership| membership.org_id == target_org_id)
            .ok_or_else(|| Error::forbidden("not a member of this organization"))?;
        state
            .iam
            .service
            .iam_memberships
            .assigned_roles(&context.user_id, &target_org_id)
            .await?
    };
    let display_role = roles
        .iter()
        .map(|role| role.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let token = state
        .iam
        .service
        .issue_token(&context.user_id, &target_org_id)?;
    Ok(Json(SelectOrgResp {
        token,
        user_id: context.user_id.0,
        org_id: target_org_id.0,
        org_name: organization.name,
        display_role,
        roles,
        system: false,
    }))
}

async fn select_system_organization(
    state: AppState,
    context: IamContext,
    target_org_id: Id,
    organization: Organization,
) -> Result<Json<SelectOrgResp>> {
    let role = system_assigned_role(&state).await?;
    let display_role = role.name.clone();
    let token = state
        .iam
        .service
        .issue_system_token(&context.user_id, &target_org_id)?;
    let _ = state
        .iam
        .audit_events
        .record(AuditEvent {
            id: Id::new(),
            org_id: state.iam.system_org_id.clone(),
            actor_kind: "user".into(),
            actor_id: context.user_id.0.clone(),
            action: "system_scope.issue".into(),
            target_kind: Some("organization".into()),
            target_id: Some(target_org_id.0.clone()),
            ip: None,
            user_agent: None,
            payload: serde_json::json!({
                "max_lifetime_seconds": 3600,
                "display_role": display_role,
                "role_id": role.id.0,
            }),
            ts: TimestampMicros::now(),
        })
        .await;
    Ok(Json(SelectOrgResp {
        token,
        user_id: context.user_id.0,
        org_id: target_org_id.0,
        org_name: organization.name,
        display_role,
        roles: vec![role],
        system: true,
    }))
}
