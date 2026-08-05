// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Unified IAM access contracts.
//!
//! These types describe the storage-independent IAM access model used by
//! capability snapshots, resource decisions, IAM APIs, and persistence.

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
    pub user_id: Id,
    pub resource_type: String,
    pub resource_id: String,
    pub permission: String,
    pub now: TimestampMicros,
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
    async fn role_for_purpose(
        &self,
        organization_id: &Id,
        purpose: &str,
    ) -> Result<Option<IamAssignedRole>>;
    async fn validate_permission_keys(
        &self,
        permission_keys: &[String],
        expected_scope: IamPermissionScope,
    ) -> Result<Vec<String>>;

    async fn policy_version(&self, organization_id: &Id) -> Result<u64>;
    async fn membership_exists(&self, organization_id: &Id, user_id: &Id) -> Result<bool>;
    async fn active_role_bindings(
        &self,
        organization_id: &Id,
        user_id: &Id,
        now: TimestampMicros,
    ) -> Result<Vec<ResolvedIamRoleBinding>>;
    async fn list_role_bindings(&self, organization_id: &Id) -> Result<Vec<IamRoleBinding>>;
    async fn create_role_binding(&self, binding: IamRoleBinding) -> Result<(IamRoleBinding, u64)>;
    async fn delete_role_binding(&self, organization_id: &Id, binding_id: &Id) -> Result<u64>;

    async fn list_relationships(
        &self,
        organization_id: &Id,
    ) -> Result<Vec<IamResourceRelationship>>;
    async fn matching_relationships(
        &self,
        organization_id: &Id,
        user_id: &Id,
        resource_type: &str,
        resource_id: &str,
        container_type: Option<&str>,
        container_id: Option<&str>,
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
