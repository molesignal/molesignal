// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Versioned IAM capability snapshots and deterministic access decisions.

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use super::IamContext;
use crate::{
    domain::iam::{
        IamAssignedRole, IamPlatformAdministratorRepository, IamScope, access::IamRepository,
        navigation::IamRouteAccess,
    },
    shared::{LicenseGate, Result, ids::Id},
};

mod capabilities;
mod evaluation;

pub use evaluation::validate_iam_conditions;

#[derive(Debug, Clone)]
pub struct IamSubject {
    pub user_id: Id,
    pub organization_id: Id,
    pub credential_role_id: Option<Id>,
    pub credential_application_id: Option<String>,
    pub scope: IamScope,
}

impl From<&IamContext> for IamSubject {
    fn from(context: &IamContext) -> Self {
        Self {
            user_id: context.user_id.clone(),
            organization_id: context.org_id.clone(),
            credential_role_id: context.credential_role_id.clone(),
            credential_application_id: context.credential_application_id.clone(),
            scope: context.scope,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IamCapabilitySnapshot {
    pub organization_id: String,
    pub scope: IamScope,
    pub display_role: String,
    pub roles: Vec<IamAssignedRole>,
    pub permissions: Vec<String>,
    pub features: Vec<String>,
    pub version: u64,
    pub route_catalog_version: u64,
    pub routes: Vec<IamRouteAccess>,
}

impl IamCapabilitySnapshot {
    pub fn has(&self, permission: &str) -> bool {
        self.permissions
            .binary_search_by(|candidate| candidate.as_str().cmp(permission))
            .is_ok()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IamAttributes {
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IamTarget {
    #[serde(default)]
    pub organization_id: Option<String>,
    pub resource_type: String,
    pub resource_id: String,
    #[serde(default)]
    pub container_type: Option<String>,
    #[serde(default)]
    pub container_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IamAccessRequest {
    pub permission: String,
    #[serde(default)]
    pub target: Option<IamTarget>,
    #[serde(default)]
    pub attributes: IamAttributes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IamDecisionReason {
    PlatformAssignment,
    RolePermission,
    ResourceRoleBinding,
    ResourceRelationship,
    CrossOrganizationGrant,
    ScopeBoundary,
    TenantIsolation,
    MissingMembership,
    ConditionMismatch,
    DefaultDeny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IamDecision {
    pub allowed: bool,
    pub reason: IamDecisionReason,
    pub policy_version: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched_binding_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched_relationship_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched_grant_ids: Vec<String>,
}

impl IamDecision {
    fn allow(reason: IamDecisionReason, policy_version: u64) -> Self {
        Self {
            allowed: true,
            reason,
            policy_version,
            matched_binding_ids: Vec::new(),
            matched_relationship_ids: Vec::new(),
            matched_grant_ids: Vec::new(),
        }
    }

    fn deny(reason: IamDecisionReason, policy_version: u64) -> Self {
        Self {
            allowed: false,
            reason,
            policy_version,
            matched_binding_ids: Vec::new(),
            matched_relationship_ids: Vec::new(),
            matched_grant_ids: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SnapshotCacheKey {
    organization_id: String,
    user_id: String,
    scope: &'static str,
    credential_role_id: Option<String>,
    credential_application_id: Option<String>,
    version: u64,
    permission_catalog_version: u64,
    route_catalog_version: u64,
    license_features: Vec<String>,
    root: bool,
}

pub struct IamAccessService {
    repository: Arc<dyn IamRepository>,
    iam_platform_administrators: Arc<dyn IamPlatformAdministratorRepository>,
    license: Arc<dyn LicenseGate>,
    snapshots: DashMap<SnapshotCacheKey, Arc<IamCapabilitySnapshot>>,
}

#[async_trait]
pub trait IamContextEnricher: Send + Sync {
    async fn enrich_iam_context(&self, context: &mut IamContext) -> Result<()>;
}

#[async_trait]
impl IamContextEnricher for IamAccessService {
    async fn enrich_iam_context(&self, context: &mut IamContext) -> Result<()> {
        self.enrich_context(context).await.map(|_| ())
    }
}

impl IamAccessService {
    pub fn new(
        repository: Arc<dyn IamRepository>,
        iam_platform_administrators: Arc<dyn IamPlatformAdministratorRepository>,
        license: Arc<dyn LicenseGate>,
    ) -> Self {
        Self {
            repository,
            iam_platform_administrators,
            license,
            snapshots: DashMap::new(),
        }
    }

    pub fn repository(&self) -> &Arc<dyn IamRepository> {
        &self.repository
    }

    /// Resolve and attach the current server-side snapshot to a request context.
    pub async fn enrich_context(&self, context: &mut IamContext) -> Result<IamCapabilitySnapshot> {
        let snapshot = self.capabilities(&IamSubject::from(&*context)).await?;
        context.permissions = snapshot.permissions.iter().cloned().collect();
        context.features = snapshot.features.iter().cloned().collect();
        context.display_role = snapshot.display_role.clone();
        context.roles = snapshot.roles.clone();
        context.policy_version = snapshot.version;
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::{
        domain::iam::access::{IamPrincipalType, IamRoleBinding},
        shared::time::TimestampMicros,
    };

    #[test]
    fn bounded_conditions_reject_unknown_keys_and_match_environment_and_labels() {
        let attributes = IamAttributes {
            environment: Some("prod".into()),
            labels: BTreeMap::from([("team".into(), "sre".into())]),
        };
        assert!(evaluation::conditions_match(
            &serde_json::json!({"environment": "prod", "labels": {"team": "sre"}}),
            &attributes
        ));
        assert!(!evaluation::conditions_match(
            &serde_json::json!({"environment": "dev"}),
            &attributes
        ));
        assert!(!evaluation::conditions_match(
            &serde_json::json!({"script": "allow()"}),
            &attributes
        ));
    }

    #[test]
    fn resource_binding_is_bounded_to_declared_type_and_id() {
        let binding = IamRoleBinding {
            id: Id::from_string("b1"),
            organization_id: Id::from_string("o1"),
            role_id: Id::from_string("r1"),
            principal_type: IamPrincipalType::User,
            principal_id: Id::from_string("u1"),
            resource_type: Some("dashboard".into()),
            resource_id: Some("d1".into()),
            conditions: Value::Null,
            starts_at: None,
            expires_at: None,
            created_by: Id::from_string("u1"),
            created_at: TimestampMicros(1),
        };
        let target = IamTarget {
            organization_id: None,
            resource_type: "dashboard".into(),
            resource_id: "d1".into(),
            container_type: None,
            container_id: None,
        };
        assert!(evaluation::binding_matches_target(&binding, &target));
        assert!(!evaluation::binding_matches_target(
            &binding,
            &IamTarget {
                resource_id: "d2".into(),
                ..target
            }
        ));
    }
}
