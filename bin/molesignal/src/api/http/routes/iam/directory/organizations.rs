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
    domain::iam::{
        IamAssignedRole, IamMembership, IamScope, ORGANIZATION_BOOTSTRAP_ROLE_PURPOSE,
        Organization, PLATFORM_ADMINISTRATOR_ROLE_PURPOSE,
    },
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
    /// Whether the caller can enter the organization. From the platform organization an owner
    /// administers organizations they do not belong to: those are listed, and cannot be entered.
    accessible: bool,
}

impl OrgView {
    fn from_org(organization: Organization, roles: Vec<IamAssignedRole>, accessible: bool) -> Self {
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
            accessible,
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
        .role_id_for_purpose(organization_id, ORGANIZATION_BOOTSTRAP_ROLE_PURPOSE)
        .await?;

    state
        .iam
        .access
        .repository()
        .role_summary(organization_id, &role_id)
        .await?
        .ok_or_else(|| Error::internal("organization root role was not materialized"))
}

/// Whether the caller is the configured root, who reaches every organization as its owner.
async fn is_root(state: &AppState, context: &IamContext) -> Result<bool> {
    Ok(context.scope != IamScope::ApiToken
        && state
            .iam
            .platform_administrators
            .is_active(&context.user_id)
            .await?)
}

/// Whether the caller may enter the platform organization: root, or an organization's owner,
/// who holds the platform permissions with the role. An API token never does.
async fn has_platform_access(state: &AppState, context: &IamContext, root: bool) -> Result<bool> {
    if root {
        return Ok(true);
    }

    if context.scope == IamScope::ApiToken {
        return Ok(false);
    }

    state
        .iam
        .access
        .holds_platform_permissions(&context.user_id)
        .await
}

/// The roles the caller has in an organization, as the organization list shows them: the owner
/// role for root, the platform role in the platform organization, and otherwise what was
/// assigned. No roles means the caller administers the organization without belonging to it.
async fn caller_roles(
    state: &AppState,
    context: &IamContext,
    root: bool,
    organization: &Organization,
) -> Result<Vec<IamAssignedRole>> {
    if organization.system {
        return Ok(vec![system_assigned_role(state).await?]);
    }

    if root {
        return Ok(vec![organization_root_role(state, &organization.id).await?]);
    }

    state
        .iam
        .service
        .iam_memberships
        .assigned_roles(&context.user_id, &organization.id)
        .await
}

async fn list_orgs(
    State(state): State<AppState>,
    axum::Extension(context): axum::Extension<IamContext>,
) -> Result<Json<Vec<OrgView>>> {
    let root = is_root(&state, &context).await?;

    // From the platform organization the list is the one that is administered: every
    // organization there is, whoever belongs to it.
    if context.scope == IamScope::System {
        require_system_organization_management(&state.iam.system_org_id, &context)?;
    }

    // The organizations the caller belongs to, once each, in the order they were joined.
    let mut memberships = Vec::new();

    if !root {
        let mut seen = HashSet::new();

        for membership in state
            .iam
            .service
            .iam_memberships
            .list_for_user(&context.user_id)
            .await?
        {
            if seen.insert(membership.org_id.clone()) {
                memberships.push(membership.org_id);
            }
        }
    }

    if root || context.scope == IamScope::System {
        let mut views = Vec::new();

        for organization in state.iam.service.orgs.list().await? {
            let accessible = root || organization.system || memberships.contains(&organization.id);
            let roles = caller_roles(&state, &context, root, &organization).await?;
            views.push(OrgView::from_org(organization, roles, accessible));
        }

        return Ok(Json(views));
    }

    let mut organizations = Vec::new();

    for organization_id in &memberships {
        if let Ok(organization) = state.iam.service.orgs.get(organization_id).await {
            let roles = caller_roles(&state, &context, root, &organization).await?;
            organizations.push(OrgView::from_org(organization, roles, true));
        }
    }

    // An owner holds the platform permissions, and uses them in the platform organization:
    // it is listed so that it can be entered.
    if has_platform_access(&state, &context, root).await? {
        let system = state.iam.service.orgs.get(&state.iam.system_org_id).await?;
        let roles = caller_roles(&state, &context, root, &system).await?;
        organizations.push(OrgView::from_org(system, roles, true));
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

    let root = is_root(&state, &context).await?;

    // Root reaches every organization without belonging to it. Anyone else who creates one
    // becomes its first member and owner: otherwise nobody could get in to set it up.
    if !root {
        let owner_role_id = state
            .iam
            .service
            .iam_memberships
            .role_id_for_purpose(&organization.id, ORGANIZATION_BOOTSTRAP_ROLE_PURPOSE)
            .await?;

        state
            .iam
            .service
            .iam_memberships
            .upsert(
                IamMembership {
                    user_id: context.user_id.clone(),
                    org_id: organization.id.clone(),
                    joined_at: now,
                },
                &[owner_role_id],
                &context.user_id,
            )
            .await?;
    }

    let roles = caller_roles(&state, &context, root, &organization).await?;

    Ok((
        StatusCode::CREATED,
        Json(OrgView::from_org(organization, roles, true)),
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

    let root = is_root(&state, &context).await?;
    let roles = caller_roles(&state, &context, root, &organization).await?;
    // No roles here means the caller administers the organization without belonging to it.
    let accessible = root || !roles.is_empty();
    Ok(Json(OrgView::from_org(organization, roles, accessible)))
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
            actor_id: context.user_id.0.clone(),
            action: action.into(),
            target_kind: Some("organization".into()),
            target_id: Some(target_org_id.0),
            ip: None,
            user_agent: None,
            payload: serde_json::json!({ "disabled": request.disabled }),
            ts: TimestampMicros::now(),
        })
        .await;

    let root = is_root(&state, &context).await?;
    let roles = caller_roles(&state, &context, root, &organization).await?;
    // No roles here means the caller administers the organization without belonging to it.
    let accessible = root || !roles.is_empty();
    Ok(Json(OrgView::from_org(organization, roles, accessible)))
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

    let root = is_root(&state, &context).await?;

    if organization.system {
        // Hidden from everyone who cannot enter it, as any organization one is not part of.
        if !has_platform_access(&state, &context, root).await? {
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
