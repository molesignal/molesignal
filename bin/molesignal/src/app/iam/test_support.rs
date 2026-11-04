// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 单测共用：一个只签发和校验 token 的 [`IamService`]，不读任何仓储；一个次数记在内存里的
//! [`SigninLimiter`]。

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use base64::Engine as _;

use super::{IamService, signin_limit::SigninLimiter};
use crate::{
    config::AuthSettings,
    domain::iam::{
        ClientIpResolverSettings, IamAssignedRole, IamMembership, IamMembershipRepository,
        InstanceSettings, InstanceSettingsRepository, Organization, OrganizationRepository,
        SigninLimits, User, UserRepository, UserStatus,
    },
    infra::{
        persistence::repositories::audit_events::{
            AuditEvent, AuditEventRepository, AuditQuery, AuditRangeSummary,
        },
        signin::MemoryAttempts,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// [`signin_limiter`] 的平台组织。
pub(crate) const SYSTEM_ORG: &str = "system-org";

/// 一次登录持续 `session_ttl_secs` 的 [`IamService`]。
pub(crate) fn iam_service(session_ttl_secs: u64) -> IamService {
    IamService::new(
        Arc::new(NoDirectory),
        Arc::new(NoDirectory),
        Arc::new(NoDirectory),
        AuthSettings {
            deprecated_jwt_secret: None,
            session_ttl_secs,
            root_email: String::new(),
            root_password: String::new(),
        },
        vec![b"iam-test-support-secret".to_vec()],
    )
}

/// 用这些账号登录的 [`IamService`]。
pub(crate) fn iam_service_with(users: Arc<dyn UserRepository>) -> IamService {
    IamService::new(
        users,
        Arc::new(NoDirectory),
        Arc::new(NoDirectory),
        AuthSettings {
            deprecated_jwt_secret: None,
            session_ttl_secs: 3600,
            root_email: String::new(),
            root_password: String::new(),
        },
        vec![b"iam-test-support-secret".to_vec()],
    )
}

/// 按这些限额、只有这些账号的 [`SigninLimiter`]，和它写的审计。
pub(crate) fn signin_limiter(
    limits: SigninLimits,
    users: Vec<User>,
) -> (SigninLimiter, Arc<AuditLog>) {
    let audit = Arc::new(AuditLog::default());

    let limiter = SigninLimiter::new(
        Arc::new(MemoryAttempts::default()),
        Arc::new(InstanceLimits(limits)),
        Arc::new(Accounts(users)),
        audit.clone(),
        Id::from_string(SYSTEM_ORG),
    );

    (limiter, audit)
}

/// 记下写进来的审计记录。
#[derive(Default)]
pub(crate) struct AuditLog(Mutex<Vec<AuditEvent>>);

impl AuditLog {
    pub(crate) fn events(&self) -> Vec<AuditEvent> {
        self.0.lock().unwrap().clone()
    }
}

#[async_trait]
impl AuditEventRepository for AuditLog {
    async fn record(&self, event: AuditEvent) -> Result<()> {
        self.0.lock().unwrap().push(event);
        Ok(())
    }
    async fn list_recent(&self, _: &Id, _: i64) -> Result<Vec<AuditEvent>> {
        Ok(Vec::new())
    }
    async fn query(&self, _: &Id, _: &AuditQuery) -> Result<Vec<AuditEvent>> {
        Ok(Vec::new())
    }
    async fn delete_before(&self, _: TimestampMicros, _: i64) -> Result<u64> {
        Ok(0)
    }
    async fn summarize_range(
        &self,
        _: Option<TimestampMicros>,
        _: TimestampMicros,
    ) -> Result<AuditRangeSummary> {
        Ok(AuditRangeSummary::default())
    }
}

/// 只有登录限额有意义的实例设置。
struct InstanceLimits(SigninLimits);

#[async_trait]
impl InstanceSettingsRepository for InstanceLimits {
    async fn get(&self) -> Result<InstanceSettings> {
        Ok(InstanceSettings {
            signup_enabled: false,
            signup_require_approval: true,
            service_graph_source: "intake".into(),
            federation_cluster_id: String::new(),
            federation_drain_interval_secs: 10,
            federation_push_batch_size: 100,
            federation_seen_events_ttl_secs: 604_800,
            federation_gossip_interval_secs: 60,
            rum_client_ip_resolver: ClientIpResolverSettings::default(),
            audit_retention_days: 365,
            default_retention_days: 30,
            synthetic_result_retention_days: 30,
            max_active_series_per_org: 200_000,
            max_active_series_per_metric: 100_000,
            apm_limits: Default::default(),
            alert_evaluation_delay_secs: 60,
            intake_usage_retention_days: 90,
            intake_usage_max_services_per_org: 1000,
            signin_limits: self.0,
            updated_at: TimestampMicros(0),
        })
    }
    async fn update(&self, settings: InstanceSettings) -> Result<InstanceSettings> {
        Ok(settings)
    }
}

/// token 从签发到过期的秒数（`exp - iat`）。
pub(crate) fn token_lifetime_secs(token: &str) -> u64 {
    let payload = token.split('.').nth(1).expect("a JWT has three parts");

    let claims: serde_json::Value = serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .expect("JWT payload is base64url"),
    )
    .expect("JWT payload is JSON");

    claims["exp"].as_u64().expect("exp") - claims["iat"].as_u64().expect("iat")
}

/// 没有任何用户、组织和成员关系的目录。
struct NoDirectory;

fn empty() -> Error {
    Error::not_found("the test directory is empty")
}

#[async_trait]
impl UserRepository for NoDirectory {
    async fn create(&self, _: User) -> Result<User> {
        Err(empty())
    }
    async fn get(&self, _: &Id) -> Result<User> {
        Err(empty())
    }
    async fn get_by_email(&self, _: &str) -> Result<User> {
        Err(empty())
    }
    async fn update(&self, _: User) -> Result<User> {
        Err(empty())
    }
    async fn delete(&self, _: &Id) -> Result<()> {
        Err(empty())
    }
    async fn count(&self) -> Result<u64> {
        Ok(0)
    }
    async fn list(&self) -> Result<Vec<User>> {
        Ok(Vec::new())
    }
    async fn set_status(&self, _: &Id, _: UserStatus) -> Result<()> {
        Err(empty())
    }
}

/// 只有这些账号的目录，只读。
pub(crate) struct Accounts(pub Vec<User>);

#[async_trait]
impl UserRepository for Accounts {
    async fn create(&self, _: User) -> Result<User> {
        Err(empty())
    }
    async fn get(&self, id: &Id) -> Result<User> {
        self.0
            .iter()
            .find(|user| &user.id == id)
            .cloned()
            .ok_or_else(empty)
    }
    async fn get_by_email(&self, email: &str) -> Result<User> {
        self.0
            .iter()
            .find(|user| user.email == email)
            .cloned()
            .ok_or_else(empty)
    }
    async fn update(&self, _: User) -> Result<User> {
        Err(empty())
    }
    async fn delete(&self, _: &Id) -> Result<()> {
        Err(empty())
    }
    async fn count(&self) -> Result<u64> {
        Ok(self.0.len() as u64)
    }
    async fn list(&self) -> Result<Vec<User>> {
        Ok(self.0.clone())
    }
    async fn set_status(&self, _: &Id, _: UserStatus) -> Result<()> {
        Err(empty())
    }
}

#[async_trait]
impl OrganizationRepository for NoDirectory {
    async fn create(&self, _: Organization) -> Result<Organization> {
        Err(empty())
    }
    async fn get(&self, _: &Id) -> Result<Organization> {
        Err(empty())
    }
    async fn get_by_slug(&self, _: &str) -> Result<Organization> {
        Err(empty())
    }
    async fn list(&self) -> Result<Vec<Organization>> {
        Ok(Vec::new())
    }
    async fn update_name(&self, _: &Id, _: String) -> Result<Organization> {
        Err(empty())
    }
    async fn set_disabled(&self, _: &Id, _: bool) -> Result<Organization> {
        Err(empty())
    }
    async fn delete(&self, _: &Id) -> Result<()> {
        Err(empty())
    }
}

#[async_trait]
impl IamMembershipRepository for NoDirectory {
    async fn upsert(&self, _: IamMembership, _: &[Id], _: &Id) -> Result<()> {
        Err(empty())
    }
    async fn list_for_user(&self, _: &Id) -> Result<Vec<IamMembership>> {
        Ok(Vec::new())
    }
    async fn list_for_org(&self, _: &Id) -> Result<Vec<IamMembership>> {
        Ok(Vec::new())
    }
    async fn assigned_roles(&self, _: &Id, _: &Id) -> Result<Vec<IamAssignedRole>> {
        Ok(Vec::new())
    }
    async fn role_id_for_purpose(&self, _: &Id, _: &str) -> Result<Id> {
        Err(empty())
    }
    async fn remove(&self, _: &Id, _: &Id) -> Result<()> {
        Err(empty())
    }
}
