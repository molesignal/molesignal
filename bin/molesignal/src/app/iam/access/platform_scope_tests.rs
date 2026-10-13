// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Who holds platform permissions, and where they take effect.

use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;

use super::{
    IamAccessRequest, IamAccessService, IamAttributes, IamDecisionReason, IamSubject, IamTarget,
};
use crate::{
    app::iam::IamContext,
    domain::iam::{
        IamAssignedRole, IamPlatformAdministrator, IamPlatformAdministratorRepository, IamScope,
        access::{
            IamCrossOrgGrant, IamCrossOrgGrantQuery, IamCrossOrgGrantStatus, IamPrincipalType,
            IamRelationshipQuery, IamRepository, IamResourceRelationship, IamRoleBinding,
            ResolvedIamResourceRelationship, ResolvedIamRoleBinding,
        },
        catalog::{IamPermissionCatalog, IamPermissionDefinition, IamPermissionScope},
        navigation::IamRouteCatalog,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const SYSTEM_ORG: &str = "_sys-id";
const TENANT: &str = "tenant-a";
const ROOT: &str = "root-user";
const OWNER: &str = "owner-user";
const PLATFORM_KEYS: [&str; 2] = ["sys.organizations.manage", "sys.telemetry.read"];
const ORGANIZATION_KEYS: [&str; 2] = ["dashboards.read", "streams.query"];

/// An IAM database reduced to what a capability snapshot and a decision read.
struct FakeIam {
    /// What `platform_permissions` answers for the owner; emptied to take the role away.
    owner_grant: Mutex<Vec<String>>,
}

impl FakeIam {
    fn new() -> Self {
        Self {
            owner_grant: Mutex::new(PLATFORM_KEYS.map(String::from).to_vec()),
        }
    }
}

fn definition(key: &str, scope: IamPermissionScope) -> IamPermissionDefinition {
    IamPermissionDefinition {
        key: key.into(),
        scope,
        domain: "test".into(),
        label_key: String::new(),
        description_key: String::new(),
        builtin_roles: Vec::new(),
    }
}

fn role(key: &str, name: &str) -> IamAssignedRole {
    IamAssignedRole {
        id: Id::from_string(format!("{key}-id")),
        key: key.into(),
        name: name.into(),
        builtin: true,
    }
}

#[async_trait]
impl IamRepository for FakeIam {
    async fn permission_catalog_version(&self) -> Result<u64> {
        Ok(1)
    }

    async fn permission_catalog(&self) -> Result<IamPermissionCatalog> {
        Ok(IamPermissionCatalog {
            version: 1,
            permissions: PLATFORM_KEYS
                .iter()
                .map(|key| definition(key, IamPermissionScope::Platform))
                .chain(
                    ORGANIZATION_KEYS
                        .iter()
                        .map(|key| definition(key, IamPermissionScope::Organization)),
                )
                .collect(),
            bundles: Vec::new(),
        })
    }

    async fn route_catalog_version(&self) -> Result<u64> {
        Ok(1)
    }

    async fn route_catalog(&self) -> Result<IamRouteCatalog> {
        Ok(IamRouteCatalog {
            version: 1,
            routes: Vec::new(),
        })
    }

    async fn permission_scope(&self, permission_key: &str) -> Result<Option<IamPermissionScope>> {
        Ok(if PLATFORM_KEYS.contains(&permission_key) {
            Some(IamPermissionScope::Platform)
        } else if ORGANIZATION_KEYS.contains(&permission_key) {
            Some(IamPermissionScope::Organization)
        } else {
            None
        })
    }

    async fn role_for_purpose(
        &self,
        organization_id: &Id,
        _purpose: &str,
    ) -> Result<Option<IamAssignedRole>> {
        Ok(Some(if organization_id.0 == SYSTEM_ORG {
            role("platform_owner", "Platform owner")
        } else {
            role("owner", "Owner")
        }))
    }

    async fn policy_version(&self, _organization_id: &Id) -> Result<u64> {
        Ok(1)
    }

    async fn platform_permissions(
        &self,
        user_id: &Id,
        _now: TimestampMicros,
    ) -> Result<Vec<String>> {
        Ok(if user_id.0 == OWNER {
            self.owner_grant.lock().clone()
        } else {
            Vec::new()
        })
    }

    async fn role_permissions(&self, _: &Id, _: &Id) -> Result<Vec<String>> {
        unreachable!("not read by these tests")
    }

    async fn role_summary(&self, _: &Id, _: &Id) -> Result<Option<IamAssignedRole>> {
        unreachable!("not read by these tests")
    }

    async fn role_summaries(&self, _: &Id, _: &[Id]) -> Result<Vec<IamAssignedRole>> {
        unreachable!("not read by these tests")
    }

    async fn validate_permission_keys(
        &self,
        _: &[String],
        _: IamPermissionScope,
    ) -> Result<Vec<String>> {
        unreachable!("not read by these tests")
    }

    async fn membership_exists(&self, _: &Id, _: &Id) -> Result<bool> {
        unreachable!("not read by these tests")
    }

    async fn active_role_bindings(
        &self,
        _: &Id,
        _: IamPrincipalType,
        _: &Id,
        _: TimestampMicros,
    ) -> Result<Vec<ResolvedIamRoleBinding>> {
        unreachable!("not read by these tests")
    }

    async fn list_role_bindings(&self, _: &Id) -> Result<Vec<IamRoleBinding>> {
        unreachable!("not read by these tests")
    }

    async fn create_role_binding(&self, _: IamRoleBinding) -> Result<(IamRoleBinding, u64)> {
        unreachable!("not read by these tests")
    }

    async fn delete_role_binding(&self, _: &Id, _: &Id) -> Result<u64> {
        unreachable!("not read by these tests")
    }

    async fn list_relationships(&self, _: &Id) -> Result<Vec<IamResourceRelationship>> {
        unreachable!("not read by these tests")
    }

    async fn matching_relationships(
        &self,
        _: &IamRelationshipQuery,
    ) -> Result<Vec<ResolvedIamResourceRelationship>> {
        unreachable!("not read by these tests")
    }

    async fn create_relationship(
        &self,
        _: IamResourceRelationship,
    ) -> Result<(IamResourceRelationship, u64)> {
        unreachable!("not read by these tests")
    }

    async fn delete_relationship(&self, _: &Id, _: &Id) -> Result<u64> {
        unreachable!("not read by these tests")
    }

    async fn list_cross_org_grants(&self, _: &Id) -> Result<Vec<IamCrossOrgGrant>> {
        unreachable!("not read by these tests")
    }

    async fn matching_cross_org_grants(
        &self,
        _: &IamCrossOrgGrantQuery,
    ) -> Result<Vec<IamCrossOrgGrant>> {
        unreachable!("not read by these tests")
    }

    async fn create_cross_org_grant(&self, _: IamCrossOrgGrant) -> Result<(IamCrossOrgGrant, u64)> {
        unreachable!("not read by these tests")
    }

    async fn set_cross_org_grant_status(
        &self,
        _: &Id,
        _: &Id,
        _: IamCrossOrgGrantStatus,
        _: &Id,
        _: TimestampMicros,
    ) -> Result<(IamCrossOrgGrant, u64)> {
        unreachable!("not read by these tests")
    }
}

struct OneRoot;

#[async_trait]
impl IamPlatformAdministratorRepository for OneRoot {
    async fn list(&self) -> Result<Vec<IamPlatformAdministrator>> {
        unreachable!("not read by these tests")
    }

    async fn is_active(&self, user_id: &Id) -> Result<bool> {
        Ok(user_id.0 == ROOT)
    }

    async fn bootstrap_root(&self, _: &Id) -> Result<bool> {
        unreachable!("not read by these tests")
    }
}

fn service() -> (IamAccessService, Arc<FakeIam>) {
    let iam = Arc::new(FakeIam::new());
    (IamAccessService::new(iam.clone(), Arc::new(OneRoot)), iam)
}

fn subject(user: &str, organization: &str, scope: IamScope) -> IamSubject {
    IamSubject {
        user_id: Id::from_string(user),
        organization_id: Id::from_string(organization),
        credential_role_id: None,
        credential_application_id: None,
        credential_service_account_id: None,
        scope,
    }
}

fn context(organization: &str, scope: IamScope, permissions: &[&str]) -> IamContext {
    IamContext {
        user_id: Id::from_string(OWNER),
        org_id: Id::from_string(organization),
        display_role: String::new(),
        roles: Vec::new(),
        credential_role_id: None,
        credential_application_id: None,
        credential_service_account_id: None,
        scope,
        permissions: permissions.iter().map(|key| (*key).to_owned()).collect(),
        policy_version: 1,
    }
}

fn request(permission: &str, organization: Option<&str>) -> IamAccessRequest {
    IamAccessRequest {
        permission: permission.into(),
        target: organization.map(|organization| IamTarget {
            organization_id: Some(organization.into()),
            resource_type: "dashboard".into(),
            resource_id: "d1".into(),
            container_type: None,
            container_id: None,
        }),
        attributes: IamAttributes::default(),
    }
}

#[tokio::test]
async fn root_holds_the_whole_catalog_in_an_organization_as_its_owner_does() {
    let (access, _) = service();

    let snapshot = access
        .capabilities(&subject(ROOT, TENANT, IamScope::Organization))
        .await
        .unwrap();

    for key in PLATFORM_KEYS.iter().chain(ORGANIZATION_KEYS.iter()) {
        assert!(snapshot.has(key), "root lacks {key} in an organization");
    }
    assert_eq!(snapshot.display_role, "Owner");
}

#[tokio::test]
async fn the_platform_organization_carries_only_platform_permissions() {
    let (access, _) = service();

    let snapshot = access
        .capabilities(&subject(ROOT, SYSTEM_ORG, IamScope::System))
        .await
        .unwrap();

    assert_eq!(
        snapshot.permissions,
        PLATFORM_KEYS.map(String::from).to_vec()
    );
}

#[tokio::test]
async fn an_owner_enters_the_platform_organization_with_what_the_role_holds() {
    let (access, _) = service();

    let snapshot = access
        .capabilities(&subject(OWNER, SYSTEM_ORG, IamScope::System))
        .await
        .unwrap();

    assert_eq!(
        snapshot.permissions,
        PLATFORM_KEYS.map(String::from).to_vec()
    );
    assert_eq!(snapshot.display_role, "Platform owner");
    assert_eq!(snapshot.roles[0].key, "platform_owner");
}

#[tokio::test]
async fn nobody_else_enters_the_platform_organization() {
    let (access, _) = service();

    let error = access
        .capabilities(&subject("viewer-user", SYSTEM_ORG, IamScope::System))
        .await
        .unwrap_err();

    assert!(matches!(error, Error::Forbidden(_)));
}

#[tokio::test]
async fn taking_the_owner_role_away_ends_an_open_platform_session() {
    let (access, iam) = service();
    let owner = subject(OWNER, SYSTEM_ORG, IamScope::System);
    access.capabilities(&owner).await.unwrap();

    // Narrowed: the snapshot resolved a moment ago is not served for the smaller grant.
    *iam.owner_grant.lock() = vec!["sys.telemetry.read".into()];
    let narrowed = access.capabilities(&owner).await.unwrap();
    assert_eq!(narrowed.permissions, vec!["sys.telemetry.read".to_string()]);

    // Gone: the token is still valid, the session is not.
    iam.owner_grant.lock().clear();
    assert!(matches!(
        access.capabilities(&owner).await.unwrap_err(),
        Error::Forbidden(_)
    ));
}

#[tokio::test]
async fn a_platform_session_reaches_the_platform_organization_and_no_tenant() {
    let (access, _) = service();
    let session = context(SYSTEM_ORG, IamScope::System, &["sys.telemetry.read"]);

    let own = access
        .evaluate(&session, &request("sys.telemetry.read", Some(SYSTEM_ORG)))
        .await
        .unwrap();
    assert!(own.allowed);
    assert_eq!(own.reason, IamDecisionReason::PlatformAssignment);

    let untargeted = access
        .evaluate(&session, &request("sys.telemetry.read", None))
        .await
        .unwrap();
    assert!(untargeted.allowed);

    // Holding the platform permission does not open a tenant's resource by its id.
    let foreign = access
        .evaluate(&session, &request("sys.telemetry.read", Some(TENANT)))
        .await
        .unwrap();
    assert!(!foreign.allowed);
    assert_eq!(foreign.reason, IamDecisionReason::TenantIsolation);

    let organization_permission = access
        .evaluate(&session, &request("dashboards.read", Some(SYSTEM_ORG)))
        .await
        .unwrap();
    assert_eq!(
        organization_permission.reason,
        IamDecisionReason::ScopeBoundary
    );
}

#[tokio::test]
async fn a_platform_permission_decides_no_resource_inside_an_organization() {
    let (access, _) = service();
    // An owner holds the key in the organization too; resources there are still decided by
    // organization permissions, so the key cannot be turned on another tenant's resource.
    let session = context(TENANT, IamScope::Organization, &["sys.telemetry.read"]);

    for target in [Some(TENANT), Some("tenant-b"), None] {
        let decision = access
            .evaluate(&session, &request("sys.telemetry.read", target))
            .await
            .unwrap();
        assert!(!decision.allowed);
        assert_eq!(decision.reason, IamDecisionReason::ScopeBoundary);
    }
}
