// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Unified IAM access contracts.
//!
//! These types describe the storage-independent IAM access model used by
//! capability snapshots, resource decisions, IAM APIs, and persistence.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    IamAssignedRole,
    catalog::{IamPermissionCatalog, IamPermissionScope},
    navigation::IamRouteCatalog,
};
use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IamPrincipalType {
    User,
    Team,
    Group,
    ServiceAccount,
    Organization,
}

impl IamPrincipalType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Team => "team",
            Self::Group => "group",
            Self::ServiceAccount => "service_account",
            Self::Organization => "organization",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IamRoleBinding {
    pub id: Id,
    pub organization_id: Id,
    pub role_id: Id,
    pub principal_type: IamPrincipalType,
    pub principal_id: Id,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    #[serde(default)]
    pub conditions: Value,
    pub starts_at: Option<TimestampMicros>,
    pub expires_at: Option<TimestampMicros>,
    pub created_by: Id,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone)]
pub struct ResolvedIamRoleBinding {
    pub binding: IamRoleBinding,
    pub role_key: String,
    pub role_name: String,
    pub role_builtin: bool,
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IamResourceRelationship {
    pub id: Id,
    pub organization_id: Id,
    pub resource_type: String,
    pub resource_id: String,
    pub role_id: Id,
    pub subject_type: IamPrincipalType,
    pub subject_id: Id,
    pub container_type: Option<String>,
    pub container_id: Option<String>,
    pub created_by: Id,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone)]
pub struct ResolvedIamResourceRelationship {
    pub relationship: IamResourceRelationship,
    pub role_key: String,
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct IamRelationshipQuery {
    pub organization_id: Id,
    pub principal_type: IamPrincipalType,
    pub principal_id: Id,
    pub resource_type: String,
    pub resource_id: String,
    pub container_type: Option<String>,
    pub container_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IamCrossOrgGrantStatus {
    Pending,
    Active,
    Revoked,
}

impl IamCrossOrgGrantStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Revoked => "revoked",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IamCrossOrgGrant {
    pub id: Id,
    pub source_organization_id: Id,
    pub target_organization_id: Id,
    pub grantee_type: IamPrincipalType,
    pub grantee_id: Id,
    pub resource_type: String,
    pub resource_selector: Value,
    pub permissions: Vec<String>,
    #[serde(default)]
    pub conditions: Value,
    pub starts_at: Option<TimestampMicros>,
    pub expires_at: Option<TimestampMicros>,
    pub status: IamCrossOrgGrantStatus,
    pub approved_by: Option<Id>,
    pub approved_at: Option<TimestampMicros>,
    pub revoked_by: Option<Id>,
    pub revoked_at: Option<TimestampMicros>,
    pub created_by: Id,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone)]
pub struct IamCrossOrgGrantQuery {
    pub source_organization_id: Id,
    pub target_organization_id: Id,
    pub principal_type: IamPrincipalType,
    pub principal_id: Id,
    pub resource_type: String,
    pub resource_id: String,
    pub permission: String,
    pub now: TimestampMicros,
}

/// The limit an API token puts on its principal's access, read again for every request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IamTokenGrant {
    /// The permissions the token is limited to: its policy's current list, or the list it
    /// was issued with. `None` when it may do whatever its principal may.
    pub ceiling: Option<BTreeSet<String>>,
    /// The role of the service account the token authenticates.
    pub service_account_role_id: Option<Id>,
}

/// The platform permissions a binding can narrow to one organization. The others are about the
/// whole instance: a binding narrowed to an organization gives none of them.
pub const ORGANIZATION_NARROWABLE_PERMISSIONS: [&str; 4] = [
    "sys.organizations.read",
    "sys.organizations.update",
    "sys.organizations.disable",
    "sys.organizations.delete",
];

/// What an account was granted through roles bound to it in the platform organization.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IamPlatformGrant {
    /// Permissions held across the platform. Only these count as holding a permission.
    pub permissions: BTreeSet<String>,
    /// Permissions held on named organizations only: permission to organization ids. What is held
    /// across the platform is not repeated here.
    pub organizations: BTreeMap<String, BTreeSet<String>>,
    /// The roles that gave any of it.
    pub roles: Vec<IamAssignedRole>,
}

impl IamPlatformGrant {
    /// Takes in what one binding gives: `organization` is the one it is narrowed to, if any.
    /// Returns whether the permission counted.
    pub fn add(&mut self, permission: String, organization: Option<String>) -> bool {
        match organization {
            None => {
                self.organizations.remove(&permission);
                self.permissions.insert(permission);
                true
            }
            Some(organization) => {
                if !ORGANIZATION_NARROWABLE_PERMISSIONS.contains(&permission.as_str()) {
                    return false;
                }

                if !self.permissions.contains(&permission) {
                    self.organizations
                        .entry(permission)
                        .or_default()
                        .insert(organization);
                }

                true
            }
        }
    }

    /// Nothing that lets the account into the platform organization.
    pub fn is_empty(&self) -> bool {
        self.permissions.is_empty() && self.organizations.is_empty()
    }

    /// Whether the permission reaches this organization, across the platform or by name.
    pub fn reaches(&self, permission: &str, organization: &Id) -> bool {
        self.permissions.contains(permission)
            || self
                .organizations
                .get(permission)
                .is_some_and(|organizations| organizations.contains(&organization.0))
    }
}

#[async_trait]
pub trait IamRepository: Send + Sync {
    async fn permission_catalog_version(&self) -> Result<u64>;
    async fn permission_catalog(&self) -> Result<IamPermissionCatalog>;
    async fn route_catalog_version(&self) -> Result<u64>;
    async fn route_catalog(&self) -> Result<IamRouteCatalog>;
    async fn permission_scope(&self, permission_key: &str) -> Result<Option<IamPermissionScope>>;
    async fn role_permissions(&self, organization_id: &Id, role_id: &Id) -> Result<Vec<String>>;
    async fn role_summary(
        &self,
        organization_id: &Id,
        role_id: &Id,
    ) -> Result<Option<IamAssignedRole>>;
    /// Resolve multiple role summaries in one organization-scoped query.
    ///
    /// Unknown IDs are omitted. Callers that persist a role reference must still treat an omitted
    /// value as an integrity error instead of silently crossing organization boundaries.
    async fn role_summaries(
        &self,
        organization_id: &Id,
        role_ids: &[Id],
    ) -> Result<Vec<IamAssignedRole>>;
    async fn validate_permission_keys(
        &self,
        permission_keys: &[String],
        expected_scope: IamPermissionScope,
    ) -> Result<Vec<String>>;

    async fn policy_version(&self, organization_id: &Id) -> Result<u64>;
    async fn membership_exists(&self, organization_id: &Id, user_id: &Id) -> Result<bool>;
    /// What an API token may still do: `None` once it is revoked or expired, or once the
    /// service account it authenticates is disabled or deleted.
    async fn api_token_grant(
        &self,
        organization_id: &Id,
        token_id: &Id,
        now: TimestampMicros,
    ) -> Result<Option<IamTokenGrant>>;
    async fn active_role_bindings(
        &self,
        organization_id: &Id,
        principal_type: IamPrincipalType,
        principal_id: &Id,
        now: TimestampMicros,
    ) -> Result<Vec<ResolvedIamRoleBinding>>;
    /// What the user was granted through roles in the platform organization. Empty for everyone
    /// else, and for root, which needs no role.
    ///
    /// This is what lets anyone but root into the platform organization: the grant is read from
    /// the database on every use, so taking the binding away takes the access away.
    async fn platform_grant(&self, user_id: &Id, now: TimestampMicros) -> Result<IamPlatformGrant>;
    async fn list_role_bindings(&self, organization_id: &Id) -> Result<Vec<IamRoleBinding>>;
    async fn create_role_binding(&self, binding: IamRoleBinding) -> Result<(IamRoleBinding, u64)>;
    async fn delete_role_binding(&self, organization_id: &Id, binding_id: &Id) -> Result<u64>;

    async fn list_relationships(
        &self,
        organization_id: &Id,
    ) -> Result<Vec<IamResourceRelationship>>;
    async fn matching_relationships(
        &self,
        query: &IamRelationshipQuery,
    ) -> Result<Vec<ResolvedIamResourceRelationship>>;
    async fn create_relationship(
        &self,
        relationship: IamResourceRelationship,
    ) -> Result<(IamResourceRelationship, u64)>;
    async fn delete_relationship(&self, organization_id: &Id, relationship_id: &Id) -> Result<u64>;

    async fn list_cross_org_grants(&self, organization_id: &Id) -> Result<Vec<IamCrossOrgGrant>>;
    async fn matching_cross_org_grants(
        &self,
        query: &IamCrossOrgGrantQuery,
    ) -> Result<Vec<IamCrossOrgGrant>>;
    async fn create_cross_org_grant(
        &self,
        grant: IamCrossOrgGrant,
    ) -> Result<(IamCrossOrgGrant, u64)>;
    async fn set_cross_org_grant_status(
        &self,
        organization_id: &Id,
        grant_id: &Id,
        status: IamCrossOrgGrantStatus,
        actor_id: &Id,
        now: TimestampMicros,
    ) -> Result<(IamCrossOrgGrant, u64)>;
}

#[cfg(test)]
mod platform_grant_tests {
    use super::*;

    fn organization(id: &str) -> Id {
        Id::from_string(id)
    }

    #[test]
    fn a_binding_across_the_platform_reaches_every_organization() {
        let mut grant = IamPlatformGrant::default();

        assert!(grant.add("sys.organizations.update".into(), None));
        assert!(grant.reaches("sys.organizations.update", &organization("a")));
        assert!(!grant.reaches("sys.organizations.delete", &organization("a")));
    }

    #[test]
    fn a_narrowed_binding_reaches_the_organization_it_names_and_is_not_holding_the_permission() {
        let mut grant = IamPlatformGrant::default();

        assert!(grant.add("sys.organizations.update".into(), Some("a".into())));
        assert!(grant.reaches("sys.organizations.update", &organization("a")));
        assert!(!grant.reaches("sys.organizations.update", &organization("b")));
        assert!(grant.permissions.is_empty());
        assert!(!grant.is_empty());
    }

    #[test]
    fn what_is_about_the_whole_instance_cannot_be_narrowed() {
        let mut grant = IamPlatformGrant::default();

        assert!(!grant.add("sys.settings.retention.manage".into(), Some("a".into())));
        assert!(!grant.add("sys.organizations.create".into(), Some("a".into())));
        assert!(grant.is_empty());
    }

    #[test]
    fn what_is_held_across_the_platform_is_not_repeated_by_name() {
        let mut narrowed_first = IamPlatformGrant::default();
        narrowed_first.add("sys.organizations.read".into(), Some("a".into()));
        narrowed_first.add("sys.organizations.read".into(), None);

        let mut whole_first = IamPlatformGrant::default();
        whole_first.add("sys.organizations.read".into(), None);
        whole_first.add("sys.organizations.read".into(), Some("a".into()));

        assert!(narrowed_first.organizations.is_empty());
        assert_eq!(narrowed_first, whole_first);
    }
}
