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
    routing::{get, patch},
};
use serde::{Deserialize, Serialize};

mod select;
use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::{
        IamAssignedRole, IamMembership, IamScope, ORGANIZATION_BOOTSTRAP_ROLE_PURPOSE,
        Organization, super_administrator,
    },
    infra::persistence::repositories::audit_events::AuditEvent,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const ORGANIZATIONS_READ: &str = "sys.organizations.read";

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/orgs", get(list_orgs).post(create_org))
        .route("/orgs/{id}", patch(update_org).delete(delete_org))
        .route("/orgs/{id}/status", patch(set_org_status))
        .merge(select::routes())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
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

/// A platform permission held across the platform, in a session of the platform organization.
fn require_platform_permission(
    system_org_id: &Id,
    ctx: &IamContext,
    permission: &str,
) -> Result<()> {
    if ctx.scope == IamScope::System
        && ctx.org_id == *system_org_id
        && ctx.has_permission(permission)
    {
        Ok(())
    } else {
        Err(Error::forbidden(format!(
            "this needs a session of the platform organization and {permission}"
        )))
    }
}

/// An action on one organization: the platform permission held across the platform, or by a binding
/// narrowed to that organization.
async fn require_organization_action(
    state: &AppState,
    ctx: &IamContext,
    permission: &str,
    target: &Id,
) -> Result<()> {
    if require_platform_permission(&state.iam.system_org_id, ctx, permission).is_ok() {
        return Ok(());
    }

    if ctx.scope == IamScope::System
        && ctx.org_id == state.iam.system_org_id
        && state
            .iam
            .access
            .platform_grant(&ctx.user_id)
            .await?
            .reaches(permission, target)
    {
        return Ok(());
    }

    Err(Error::forbidden(format!(
        "this needs {permission} on this organization"
    )))
}

/// Whether the caller is the configured root, who reaches every organization without any role.
async fn is_root(state: &AppState, context: &IamContext) -> Result<bool> {
    Ok(context.scope != IamScope::ApiToken
        && state
            .iam
            .platform_administrators
            .is_active(&context.user_id)
            .await?)
}

/// Whether the caller may enter the platform organization: root, or whoever was granted platform
/// permissions there. An API token never does.
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

/// The roles the caller has in an organization, as the organization list shows them: what was
/// granted on the platform in the platform organization, and otherwise what was assigned. Root
/// reads as itself everywhere. No roles means the caller administers the organization without
/// belonging to it.
async fn caller_roles(
    state: &AppState,
    context: &IamContext,
    root: bool,
    organization: &Organization,
) -> Result<Vec<IamAssignedRole>> {
    if root {
        return Ok(vec![super_administrator()]);
    }

    if organization.system {
        return Ok(state
            .iam
            .access
            .platform_grant(&context.user_id)
            .await?
            .roles);
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

    // From the platform organization the list is the one that is administered: every organization
    // there is, whoever belongs to it, as far as the permission to read them reaches.
    let reads_all = context.scope == IamScope::System && context.has_permission(ORGANIZATIONS_READ);

    if root || reads_all {
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

    // A binding narrowed to an organization lists it too, and does not let the caller into it.
    if context.scope == IamScope::System {
        let grant = state.iam.access.platform_grant(&context.user_id).await?;

        for organization_id in grant
            .organizations
            .get(ORGANIZATIONS_READ)
            .into_iter()
            .flatten()
        {
            let organization_id = Id::from_string(organization_id.clone());

            if memberships.contains(&organization_id) {
                continue;
            }

            if let Ok(organization) = state.iam.service.orgs.get(&organization_id).await {
                organizations.push(OrgView::from_org(organization, Vec::new(), false));
            }
        }
    }

    // Whoever was granted platform permissions uses them in the platform organization: it is
    // listed so that it can be entered.
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
    require_platform_permission(
        &state.iam.system_org_id,
        &context,
        "sys.organizations.create",
    )?;

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

    // An organization names itself from its own settings; the platform names those it reaches.
    let its_own = context.scope != IamScope::System
        && context.org_id == target_org_id
        && context.has_permission("org.preferences.manage");

    if !its_own {
        require_organization_action(&state, &context, "sys.organizations.update", &target_org_id)
            .await?;
    }

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
    let target_org_id = Id::from_string(id);
    require_organization_action(
        &state,
        &context,
        "sys.organizations.disable",
        &target_org_id,
    )
    .await?;

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

    // From inside, an organization can delete itself and no other; a token cannot do it at all.
    let deletes_itself = context.scope == IamScope::Organization
        && context.org_id == target_org_id
        && context.has_permission("org.delete");

    if !deletes_itself {
        require_organization_action(&state, &context, "sys.organizations.delete", &target_org_id)
            .await?;
    }

    let target = state.iam.service.orgs.get(&target_org_id).await?;
    target.ensure_mutable()?;

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

    // Recorded on the platform: the organization's own audit trail went with it.
    let _ = state
        .iam
        .audit_events
        .record(AuditEvent {
            id: Id::new(),
            org_id: state.iam.system_org_id.clone(),
            actor_kind: "user".into(),
            actor_id: context.user_id.0.clone(),
            action: "organization.delete".into(),
            target_kind: Some("organization".into()),
            target_id: Some(target_org_id.0),
            ip: None,
            user_agent: None,
            payload: serde_json::json!({ "name": target.name, "slug": target.slug }),
            ts: TimestampMicros::now(),
        })
        .await;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(scope: IamScope, org_id: &str, permissions: &[&str]) -> IamContext {
        IamContext {
            user_id: Id::from_string("user-1"),
            org_id: Id::from_string(org_id),
            display_role: "Platform administrator".into(),
            roles: Vec::new(),
            credential_token_id: None,
            credential_application_id: None,
            credential_service_account_id: None,
            scope,
            permissions: permissions
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            policy_version: 1,
        }
    }

    #[test]
    fn a_platform_permission_counts_in_a_platform_session_only() {
        let system_org_id = Id::from_string("_sys-id");
        let permission = "sys.organizations.create";

        assert!(
            require_platform_permission(
                &system_org_id,
                &context(IamScope::System, "_sys-id", &[permission]),
                permission
            )
            .is_ok()
        );

        for denied in [
            context(
                IamScope::Organization,
                "tenant-a",
                &[permission, "org.preferences.manage"],
            ),
            context(IamScope::System, "tenant-a", &[permission]),
            context(IamScope::System, "_sys-id", &[]),
            context(IamScope::System, "_sys-id", &["sys.organizations.update"]),
        ] {
            let error =
                require_platform_permission(&system_org_id, &denied, permission).unwrap_err();

            assert!(matches!(&error, Error::Forbidden(_)));
            assert_eq!(error.http_status_code(), 403);
        }
    }
}
