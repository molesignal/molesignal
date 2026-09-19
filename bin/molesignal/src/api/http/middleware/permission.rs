// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 权限校验 helper。
//!
//! Route handlers use `#[permission("capability.key")]` for coarse,
//! compile-time-declared checks. The declared strings reference rows in the
//! database permission catalog; no permission catalog is maintained here.
//! Persisted resources use `#[resource_permission(...)]`, which delegates to
//! the loaders and evaluators below. Truly request-dependent non-resource
//! checks may call `Permission` directly. Every decision is ultimately
//! resolved from the database IAM catalog and policy tables.

use async_trait::async_trait;

use crate::{
    api::AppState,
    app::iam::{IamAccessRequest, IamContext, IamTarget},
    shared::{Error, ids::Id},
};

pub struct Permission;

/// A persisted entity whose true organization and resource identifier are
/// loaded before resource-scoped IAM evaluation.
#[async_trait]
pub trait ProtectedResource: Sized + Send + Sync {
    type Id: Send;

    async fn load(state: &AppState, id: Self::Id) -> Result<Self, Error>;

    fn organization_id(&self) -> &Id;

    fn resource_type(&self) -> &str;

    fn resource_id(&self) -> &str;
}

/// Load a protected resource exactly once and return it only after the
/// database-backed IAM engine authorizes the requested action.
pub async fn authorize_resource<R>(
    state: &AppState,
    context: &IamContext,
    id: R::Id,
    permission: &str,
) -> Result<R, Error>
where
    R: ProtectedResource,
{
    let resource = R::load(state, id).await?;
    Permission::require_resource(
        state,
        context,
        permission,
        resource.organization_id(),
        resource.resource_type(),
        resource.resource_id(),
    )
    .await?;
    Ok(resource)
}

/// Multi-action variant used only when one resource endpoint is shared by
/// organization and platform scopes.
pub async fn authorize_resource_any<R>(
    state: &AppState,
    context: &IamContext,
    id: R::Id,
    permissions: &[&str],
) -> Result<R, Error>
where
    R: ProtectedResource,
{
    let resource = R::load(state, id).await?;
    Permission::require_any_resource(
        state,
        context,
        permissions,
        resource.organization_id(),
        resource.resource_type(),
        resource.resource_id(),
    )
    .await?;
    Ok(resource)
}

/// Resolver variant for persisted indirection records whose required database
/// permission is determined by the loaded resource itself.
pub async fn authorize_resource_with<R, F>(
    state: &AppState,
    context: &IamContext,
    id: R::Id,
    resolve_permission: F,
) -> Result<R, Error>
where
    R: ProtectedResource,
    F: FnOnce(&R) -> Result<&'static str, Error>,
{
    let resource = R::load(state, id).await?;
    let permission = resolve_permission(&resource)?;
    Permission::require_resource(
        state,
        context,
        permission,
        resource.organization_id(),
        resource.resource_type(),
        resource.resource_id(),
    )
    .await?;
    Ok(resource)
}

/// Resolver variant for mutations that require every action returned for the
/// loaded resource (for example editing a pipeline and changing its running
/// state in one request).
pub async fn authorize_resource_all_with<R, F>(
    state: &AppState,
    context: &IamContext,
    id: R::Id,
    resolve_permissions: F,
) -> Result<R, Error>
where
    R: ProtectedResource,
    F: FnOnce(&R) -> Result<Vec<&'static str>, Error>,
{
    let resource = R::load(state, id).await?;
    let permissions = resolve_permissions(&resource)?;
    if permissions.is_empty() {
        return Err(Error::invalid(
            "resource permission resolver returned no actions",
        ));
    }
    for permission in permissions {
        Permission::require_resource(
            state,
            context,
            permission,
            resource.organization_id(),
            resource.resource_type(),
            resource.resource_id(),
        )
        .await?;
    }
    Ok(resource)
}

impl Permission {
    pub fn require_key(ctx: &IamContext, permission: &str) -> Result<(), Error> {
        if ctx.has_permission(permission) {
            Ok(())
        } else {
            Err(Error::forbidden(format!(
                "scope {:?} lacks permission {permission}",
                ctx.scope
            )))
        }
    }

    pub fn require_any_key(ctx: &IamContext, permissions: &[&str]) -> Result<(), Error> {
        if permissions
            .iter()
            .any(|permission| ctx.has_permission(permission))
        {
            Ok(())
        } else {
            Err(Error::forbidden(format!(
                "scope {:?} lacks every required IAM permission: {}",
                ctx.scope,
                permissions.join(", ")
            )))
        }
    }

    /// Evaluate a concrete resource through the unified IAM engine.
    ///
    /// Foreign resources deliberately collapse denied decisions to `404` so a
    /// cross-organization request cannot probe resource existence.
    pub async fn require_resource(
        state: &AppState,
        ctx: &IamContext,
        permission: &str,
        organization_id: &Id,
        resource_type: &str,
        resource_id: &str,
    ) -> Result<(), Error> {
        let decision = state
            .iam
            .access
            .evaluate(
                ctx,
                &IamAccessRequest {
                    permission: permission.to_owned(),
                    target: Some(IamTarget {
                        organization_id: Some(organization_id.0.clone()),
                        resource_type: resource_type.to_owned(),
                        resource_id: resource_id.to_owned(),
                        container_type: None,
                        container_id: None,
                    }),
                    attributes: Default::default(),
                },
            )
            .await?;
        if decision.allowed {
            Ok(())
        } else if *organization_id != ctx.org_id {
            Err(Error::not_found(format!("{resource_type} not found")))
        } else {
            Err(Error::forbidden(format!(
                "IAM denied {permission} on {resource_type}/{resource_id}: {:?}",
                decision.reason
            )))
        }
    }

    pub async fn require_any_resource(
        state: &AppState,
        ctx: &IamContext,
        permissions: &[&str],
        organization_id: &Id,
        resource_type: &str,
        resource_id: &str,
    ) -> Result<(), Error> {
        if permissions.is_empty() {
            return Err(Error::invalid(
                "resource permission check requires at least one permission",
            ));
        }
        let mut denial_reasons = Vec::with_capacity(permissions.len());
        for permission in permissions {
            let decision = state
                .iam
                .access
                .evaluate(
                    ctx,
                    &IamAccessRequest {
                        permission: (*permission).to_owned(),
                        target: Some(IamTarget {
                            organization_id: Some(organization_id.0.clone()),
                            resource_type: resource_type.to_owned(),
                            resource_id: resource_id.to_owned(),
                            container_type: None,
                            container_id: None,
                        }),
                        attributes: Default::default(),
                    },
                )
                .await?;
            if decision.allowed {
                return Ok(());
            }
            denial_reasons.push(format!("{permission}: {:?}", decision.reason));
        }
        if *organization_id != ctx.org_id {
            Err(Error::not_found(format!("{resource_type} not found")))
        } else {
            Err(Error::forbidden(format!(
                "IAM denied every permitted action on {resource_type}/{resource_id}: {}",
                denial_reasons.join(", ")
            )))
        }
    }
}
