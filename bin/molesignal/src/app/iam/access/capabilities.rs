// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Capability snapshot resolution for interactive sessions and API tokens.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use super::{IamAccessService, IamAttributes, IamCapabilitySnapshot, IamSubject, SnapshotCacheKey};
use crate::{
    domain::iam::{
        IamAssignedRole, IamScope,
        access::{IamPlatformGrant, IamPrincipalType, IamTokenGrant},
        catalog::IamPermissionScope,
        super_administrator,
    },
    shared::{Error, Result, time::TimestampMicros},
};

impl IamAccessService {
    pub async fn capabilities(&self, subject: &IamSubject) -> Result<IamCapabilitySnapshot> {
        // A token is not root's session: see `token_access` for what root's own tokens hold.
        let root = subject.scope != IamScope::ApiToken
            && self
                .iam_platform_administrators
                .is_active(&subject.user_id)
                .await?;

        // A platform session is open to the configured root, and to whoever was granted platform
        // permissions in the platform organization. The grant is read again on every request.
        let platform_grant = if subject.scope == IamScope::System && !root {
            let grant = self.platform_grant(&subject.user_id).await?;

            if grant.is_empty() {
                return Err(Error::forbidden("platform access required"));
            }

            Some(grant)
        } else {
            None
        };

        // An API token is read again on every request too: revoking it, or disabling its service
        // account, takes effect at once even while a snapshot of it is cached.
        let token_grant = if subject.scope == IamScope::ApiToken {
            Some(self.token_grant(subject).await?)
        } else {
            None
        };

        let version = self
            .repository
            .policy_version(&subject.organization_id)
            .await?;

        let permission_catalog_version = if root {
            self.repository.permission_catalog_version().await?
        } else {
            0
        };

        let route_catalog_version = self.repository.route_catalog_version().await?;

        let mut cache_key = SnapshotCacheKey {
            organization_id: subject.organization_id.0.clone(),
            user_id: subject.user_id.0.clone(),
            scope: scope_key(subject.scope),
            credential_token_id: subject
                .credential_token_id
                .as_ref()
                .map(|token_id| token_id.0.clone()),
            credential_application_id: subject.credential_application_id.clone(),
            credential_service_account_id: subject
                .credential_service_account_id
                .as_ref()
                .map(|id| id.0.clone()),
            version,
            permission_catalog_version,
            route_catalog_version,
            root,
            platform_grant: platform_grant.as_ref().map(platform_grant_key),
            token_grant: token_grant.as_ref().map(token_grant_key),
        };

        if let Some(snapshot) = self.snapshots.get(&cache_key) {
            return Ok((**snapshot).clone());
        }

        let permission_catalog = self.repository.permission_catalog().await?;
        let route_catalog = self.repository.route_catalog().await?;

        cache_key.permission_catalog_version = if root { permission_catalog.version } else { 0 };
        cache_key.route_catalog_version = route_catalog.version;

        if let Some(snapshot) = self.snapshots.get(&cache_key) {
            return Ok((**snapshot).clone());
        }

        // A permission counts only in a session of its own scope, whoever holds it and however it
        // got there: a platform permission in the platform organization and nowhere else.
        let session_scope = match subject.scope {
            IamScope::System => IamPermissionScope::Platform,
            IamScope::Organization | IamScope::ApiToken => IamPermissionScope::Organization,
        };

        let in_session = permission_catalog
            .permissions
            .into_iter()
            .filter(|permission| permission.scope == session_scope)
            .map(|permission| permission.key)
            .collect::<BTreeSet<_>>();

        let mut organization_permissions = BTreeMap::new();

        let (permissions, roles, display_role) = if root {
            // Root holds whatever counts in the session it is in, without any role.
            let role = super_administrator();
            let display_role = role.name.clone();

            (in_session.clone(), vec![role], display_role)
        } else {
            match subject.scope {
                IamScope::System => {
                    let grant =
                        platform_grant.expect("platform grant resolved for a platform session");
                    organization_permissions = grant.organizations;

                    let display_role = grant
                        .roles
                        .iter()
                        .map(|role| role.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");

                    (grant.permissions, grant.roles, display_role)
                }
                IamScope::ApiToken => {
                    let grant = token_grant.expect("token grant resolved for an API token");
                    self.token_access(subject, grant, &in_session).await?
                }
                IamScope::Organization => {
                    self.ensure_membership(subject).await?;
                    let (permissions, roles) = self.organization_access(subject).await?;

                    let display_role = roles
                        .iter()
                        .map(|role| role.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");

                    (permissions, roles, display_role)
                }
            }
        };

        let permissions = permissions
            .intersection(&in_session)
            .cloned()
            .collect::<BTreeSet<_>>();

        // Whoever reaches a few organizations opens the pages about organizations too: what they
        // show and allow is decided organization by organization.
        let opening_pages = permissions
            .iter()
            .chain(organization_permissions.keys())
            .cloned()
            .collect::<BTreeSet<_>>();

        let routes =
            crate::app::iam::resolve_route_access(&route_catalog, subject.scope, &opening_pages);

        let snapshot = IamCapabilitySnapshot {
            organization_id: subject.organization_id.0.clone(),
            scope: subject.scope,
            display_role,
            roles,
            permissions: permissions.into_iter().collect(),
            organization_permissions: organization_permissions
                .into_iter()
                .map(|(permission, organizations)| {
                    (permission, organizations.into_iter().collect())
                })
                .collect(),
            version,
            route_catalog_version: route_catalog.version,
            routes,
        };

        self.snapshots.insert(cache_key, Arc::new(snapshot.clone()));
        Ok(snapshot)
    }

    /// What the subject's API token may still do.
    pub(super) async fn token_grant(&self, subject: &IamSubject) -> Result<IamTokenGrant> {
        let token_id = subject
            .credential_token_id
            .as_ref()
            .ok_or_else(|| Error::unauthorized("API token request names no token"))?;

        self.repository
            .api_token_grant(&subject.organization_id, token_id, TimestampMicros::now())
            .await?
            .ok_or_else(|| Error::unauthorized("API token revoked or expired"))
    }

    /// A token's access: what its principal holds, narrowed to the token's ceiling. A public RUM
    /// credential belongs to its application, which holds nothing beyond the ceiling. Root's own
    /// tokens hold what root holds in the organization they belong to; only a platform session
    /// gives the platform's permissions, and a token never is one.
    async fn token_access(
        &self,
        subject: &IamSubject,
        grant: IamTokenGrant,
        in_session: &BTreeSet<String>,
    ) -> Result<(BTreeSet<String>, Vec<IamAssignedRole>, String)> {
        if subject.credential_application_id.is_some() {
            return Ok((grant.ceiling.unwrap_or_default(), Vec::new(), String::new()));
        }

        let (held, roles) = if subject.credential_service_account_id.is_some() {
            let role_id = grant
                .service_account_role_id
                .ok_or_else(|| Error::unauthorized("service account no longer authenticates"))?;

            let role = self
                .repository
                .role_summary(&subject.organization_id, &role_id)
                .await?
                .ok_or_else(|| Error::unauthorized("service account IAM role no longer exists"))?;

            let permissions = self
                .repository
                .role_permissions(&subject.organization_id, &role_id)
                .await?
                .into_iter()
                .collect::<BTreeSet<_>>();

            (permissions, vec![role])
        } else if self
            .iam_platform_administrators
            .is_active(&subject.user_id)
            .await?
        {
            (in_session.clone(), vec![super_administrator()])
        } else {
            self.ensure_membership(subject).await?;
            self.organization_access(subject).await?
        };

        let permissions = match grant.ceiling {
            Some(ceiling) => held.intersection(&ceiling).cloned().collect(),
            None => held,
        };

        let display_role = roles
            .iter()
            .map(|role| role.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        Ok((permissions, roles, display_role))
    }

    async fn ensure_membership(&self, subject: &IamSubject) -> Result<()> {
        if self
            .repository
            .membership_exists(&subject.organization_id, &subject.user_id)
            .await?
        {
            Ok(())
        } else {
            Err(Error::forbidden("active organization membership required"))
        }
    }

    async fn organization_access(
        &self,
        subject: &IamSubject,
    ) -> Result<(BTreeSet<String>, Vec<IamAssignedRole>)> {
        let bindings = self
            .repository
            .active_role_bindings(
                &subject.organization_id,
                IamPrincipalType::User,
                &subject.user_id,
                TimestampMicros::now(),
            )
            .await?;

        let mut permissions = BTreeSet::new();
        let mut seen_roles = BTreeSet::new();
        let mut roles = Vec::new();

        for resolved in bindings {
            if resolved.binding.resource_type.is_some()
                || resolved.binding.resource_id.is_some()
                || !super::evaluation::conditions_match(
                    &resolved.binding.conditions,
                    &IamAttributes::default(),
                )
            {
                continue;
            }

            if seen_roles.insert(resolved.binding.role_id.0.clone()) {
                roles.push(IamAssignedRole {
                    id: resolved.binding.role_id.clone(),
                    key: resolved.role_key.clone(),
                    name: resolved.role_name.clone(),
                    builtin: resolved.role_builtin,
                });
            }

            permissions.extend(resolved.permissions);
        }

        Ok((permissions, roles))
    }
}

fn platform_grant_key(grant: &IamPlatformGrant) -> String {
    let roles = grant
        .roles
        .iter()
        .map(|role| role.id.0.as_str())
        .collect::<Vec<_>>()
        .join(",");

    format!("{roles}|{:?}|{:?}", grant.permissions, grant.organizations)
}

fn token_grant_key(grant: &IamTokenGrant) -> String {
    let ceiling = grant.ceiling.as_ref().map_or_else(
        || "*".to_string(),
        |ceiling| ceiling.iter().cloned().collect::<Vec<_>>().join(","),
    );

    let role = grant
        .service_account_role_id
        .as_ref()
        .map_or("", |role_id| role_id.0.as_str());

    format!("{role}|{ceiling}")
}

fn scope_key(scope: IamScope) -> &'static str {
    match scope {
        IamScope::Organization => "organization",
        IamScope::System => "system",
        IamScope::ApiToken => "api_token",
    }
}
