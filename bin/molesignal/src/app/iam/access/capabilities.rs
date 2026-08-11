// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Capability snapshot resolution for interactive sessions and API tokens.

use std::{collections::BTreeSet, sync::Arc};

use super::{IamAccessService, IamAttributes, IamCapabilitySnapshot, IamSubject, SnapshotCacheKey};
use crate::{
    domain::iam::{
        IamAssignedRole, IamScope, PLATFORM_ADMINISTRATOR_ROLE_PURPOSE, catalog::IamPermissionScope,
    },
    shared::{Error, Result, time::TimestampMicros},
};

impl IamAccessService {
    pub async fn capabilities(&self, subject: &IamSubject) -> Result<IamCapabilitySnapshot> {
        // API tokens deliberately never inherit root privileges from their issuer.
        let root = subject.scope != IamScope::ApiToken
            && self
                .iam_platform_administrators
                .is_active(&subject.user_id)
                .await?;
        if subject.scope == IamScope::System && !root {
            return Err(Error::forbidden("root system scope required"));
        }
        let features = self.license.features().into_iter().collect::<BTreeSet<_>>();
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
            credential_role_id: subject
                .credential_role_id
                .as_ref()
                .map(|role_id| role_id.0.clone()),
            credential_application_id: subject.credential_application_id.clone(),
            version,
            permission_catalog_version,
            route_catalog_version,
            license_features: features.iter().cloned().collect(),
            root,
        };
        if let Some(snapshot) = self.snapshots.get(&cache_key) {
            return Ok((**snapshot).clone());
        }

        let system_role = if subject.scope == IamScope::System {
            Some(
                self.repository
                    .role_for_purpose(
                        &subject.organization_id,
                        PLATFORM_ADMINISTRATOR_ROLE_PURPOSE,
                    )
                    .await?
                    .ok_or_else(|| {
                        Error::internal(
                            "platform administrator IAM role is not materialized for `_sys`",
                        )
                    })?,
            )
        } else {
            None
        };
        let root_permission_catalog = if root {
            Some(self.repository.permission_catalog().await?)
        } else {
            None
        };
        let route_catalog = self.repository.route_catalog().await?;
        cache_key.permission_catalog_version = root_permission_catalog
            .as_ref()
            .map_or(0, |catalog| catalog.version);
        cache_key.route_catalog_version = route_catalog.version;
        if let Some(snapshot) = self.snapshots.get(&cache_key) {
            return Ok((**snapshot).clone());
        }

        let (permissions, roles, display_role) = if root {
            let permission_scope = if subject.scope == IamScope::System {
                IamPermissionScope::Platform
            } else {
                IamPermissionScope::Organization
            };
            let permissions = root_permission_catalog
                .as_ref()
                .expect("root permission catalog resolved")
                .permissions
                .iter()
                .filter(|permission| permission.scope == permission_scope)
                .map(|permission| permission.key.clone())
                .collect::<BTreeSet<_>>();
            let role = if subject.scope == IamScope::System {
                system_role
            } else {
                self.repository
                    .role_for_purpose(&subject.organization_id, "organization_bootstrap")
                    .await?
            };
            let display_role = role
                .as_ref()
                .map(|role| role.name.clone())
                .unwrap_or_else(|| "Root".into());
            (permissions, role.into_iter().collect(), display_role)
        } else {
            match subject.scope {
                IamScope::System => {
                    return Err(Error::forbidden("root system scope required"));
                }
                IamScope::ApiToken => {
                    if subject.credential_application_id.is_none() {
                        self.ensure_membership(subject).await?;
                    }
                    let role_id = subject
                        .credential_role_id
                        .as_ref()
                        .ok_or_else(|| Error::unauthorized("API token has no IAM role"))?;
                    let role = self
                        .repository
                        .role_summary(&subject.organization_id, role_id)
                        .await?
                        .ok_or_else(|| {
                            Error::unauthorized("API token IAM role no longer exists")
                        })?;
                    let permissions = self
                        .repository
                        .role_permissions(&subject.organization_id, role_id)
                        .await?
                        .into_iter()
                        .collect();
                    let display_role = role.name.clone();
                    (permissions, vec![role], display_role)
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
        let routes = crate::app::iam::resolve_route_access(
            &route_catalog,
            subject.scope,
            &permissions,
            &features,
        );
        let snapshot = IamCapabilitySnapshot {
            organization_id: subject.organization_id.0.clone(),
            scope: subject.scope,
            display_role,
            roles,
            permissions: permissions.into_iter().collect(),
            features: features.into_iter().collect(),
            version,
            route_catalog_version: route_catalog.version,
            routes,
        };
        self.snapshots.insert(cache_key, Arc::new(snapshot.clone()));
        Ok(snapshot)
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

fn scope_key(scope: IamScope) -> &'static str {
    match scope {
        IamScope::Organization => "organization",
        IamScope::System => "system",
        IamScope::ApiToken => "api_token",
    }
}
