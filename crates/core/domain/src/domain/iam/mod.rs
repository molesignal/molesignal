// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! IAM：身份、组织上下文、成员关系、角色与权限。

use std::collections::BTreeSet;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Result, ids::Id, time::TimestampMicros};

pub mod access;
pub mod api_token;
pub mod catalog;
pub mod navigation;
pub mod policy;
pub mod service_account;
mod sso;

pub use permission_macro::{permission, resource_permission};
pub use sso::*;

pub const SYSTEM_ORG_NAME: &str = "_sys";
pub const SYSTEM_ORG_SLUG: &str = "_sys";

/// Stable business purpose of the role an organization's first member receives: its owner.
///
/// An owner holds the organization permissions of the catalog, and nothing of the platform's. That
/// is kept true by `IamRoleRepository::grant_catalog_to_organization_owners`, not by listing keys
/// per permission.
pub const ORGANIZATION_BOOTSTRAP_ROLE_PURPOSE: &str = "organization_bootstrap";

/// How root reads wherever an account's roles are shown. Root holds what counts in the session it
/// is in without any role, so this names no row of `iam_roles`.
pub fn super_administrator() -> IamAssignedRole {
    IamAssignedRole {
        id: Id::from_string("super_administrator"),
        key: "super_administrator".into(),
        name: "Super administrator".into(),
        builtin: true,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Organization {
    pub id: Id,
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub system: bool,
    #[serde(default)]
    pub disabled: bool,
    pub created_at: TimestampMicros,
}

impl Organization {
    pub fn validate_system_invariants(&self) -> Result<()> {
        let uses_reserved_system_name =
            self.name == SYSTEM_ORG_NAME || self.slug == SYSTEM_ORG_SLUG;

        if self.system
            && (self.name != SYSTEM_ORG_NAME || self.slug != SYSTEM_ORG_SLUG || self.disabled)
        {
            return Err(crate::shared::Error::invalid(
                "system organization must be enabled and its name and slug must both be `_sys`",
            ));
        }

        if !self.system && uses_reserved_system_name {
            return Err(crate::shared::Error::forbidden(
                "`_sys` is reserved for the system organization",
            ));
        }

        Ok(())
    }

    pub fn ensure_mutable(&self) -> Result<()> {
        if self.system {
            Err(crate::shared::Error::forbidden(
                "system organization is immutable",
            ))
        } else {
            Ok(())
        }
    }

    pub fn ensure_enabled(&self) -> Result<()> {
        if self.disabled {
            Err(crate::shared::Error::forbidden(
                "organization is disabled by a platform administrator",
            ))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: Id,
    pub email: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub bio: String,
    pub password_hash: String,
    pub disabled: bool,
    /// 注册审批状态：`active` 可登录；`pending` 待审批（自助注册 + 需审批时）。
    #[serde(default)]
    pub status: UserStatus,
    pub created_at: TimestampMicros,
    /// 早于这个时间开始的登录作废。只由专门的写入更新，[`UserRepository::update`] 不改它。
    #[serde(default = "sessions_always_valid")]
    pub sessions_valid_after: TimestampMicros,
}

/// 从未让登录作废过的账号。
pub fn sessions_always_valid() -> TimestampMicros {
    TimestampMicros(0)
}

/// 用户激活状态。`pending` 仅出现在「自助注册 + 需审批」流程，Owner/Admin 审批后转 `active`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserStatus {
    #[default]
    Active,
    Pending,
    /// 自助注册被管理员拒绝：不可登录，保留记录以便审计。
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Team {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub member_ids: Vec<Id>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IamMembership {
    pub user_id: Id,
    pub org_id: Id,
    pub joined_at: TimestampMicros,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IamAssignedRole {
    pub id: Id,
    pub key: String,
    pub name: String,
    pub builtin: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum IamScope {
    #[default]
    Organization,
    System,
    ApiToken,
}

/// Server-authenticated identity and authorization snapshot for one request.
///
/// Protocol adapters may read this value, but only authentication and IAM layers should construct
/// it. Downstream security-sensitive contexts derive identity from this type instead of accepting
/// request JSON or headers directly.
#[derive(Debug, Clone)]
pub struct IamContext {
    pub user_id: Id,
    pub org_id: Id,
    /// Server-resolved display metadata. Authorization never reads this field.
    pub display_role: String,
    pub roles: Vec<IamAssignedRole>,
    /// The API token the request authenticated with. Its scope bounds `permissions`.
    pub credential_token_id: Option<Id>,
    /// Public RUM credentials are bound to one application and never inherit user access.
    pub credential_application_id: Option<String>,
    /// Present only for credentials authenticating a first-class non-human principal.
    pub credential_service_account_id: Option<Id>,
    pub scope: IamScope,
    /// Canonical, server-resolved capability keys for this request.
    pub permissions: BTreeSet<String>,
    /// Monotonic organization policy version used to resolve `permissions`.
    pub policy_version: u64,
}

impl IamContext {
    pub fn is_system_scope(&self) -> bool {
        self.scope == IamScope::System
    }

    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions.contains(permission)
    }

    pub fn principal_type(&self) -> access::IamPrincipalType {
        if self.credential_service_account_id.is_some() {
            access::IamPrincipalType::ServiceAccount
        } else {
            access::IamPrincipalType::User
        }
    }

    pub fn principal_id(&self) -> &Id {
        self.credential_service_account_id
            .as_ref()
            .unwrap_or(&self.user_id)
    }

    /// Dynamic role key used only for query admission work-group selection.
    /// Authorization itself is based on `permissions`.
    pub fn organization_role_key(&self) -> &str {
        if self.is_system_scope() {
            ""
        } else {
            self.roles.first().map_or("", |role| role.key.as_str())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IamPlatformAdministrator {
    pub user_id: Id,
    pub active: bool,
    pub granted_by: Option<Id>,
    pub granted_at: TimestampMicros,
    pub revoked_by: Option<Id>,
    pub revoked_at: Option<TimestampMicros>,
}

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn create(&self, user: User) -> Result<User>;
    async fn get(&self, id: &Id) -> Result<User>;
    /// Batch lookup used by bounded directory views. Persistence adapters should
    /// override this to avoid one query per user; the default keeps test and
    /// non-database adapters source-compatible.
    async fn get_many(&self, ids: &[Id]) -> Result<Vec<User>> {
        let mut users = Vec::with_capacity(ids.len());

        for id in ids {
            users.push(self.get(id).await?);
        }

        Ok(users)
    }
    async fn get_by_email(&self, email: &str) -> Result<User>;
    async fn update(&self, user: User) -> Result<User>;
    async fn delete(&self, id: &Id) -> Result<()>;
    /// 首用户路径需要判断表是否为空。
    async fn count(&self) -> Result<u64>;
    /// admin 列出所有用户。
    async fn list(&self) -> Result<Vec<User>>;
    /// 审批自助注册：把用户 status 置为目标值（active 激活 / pending）。
    async fn set_status(&self, id: &Id, status: UserStatus) -> Result<()>;
}

/// RUM 客户端 IP 的来源。代理 Header 仅在连接对端命中可信 CIDR 时生效。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientIpMode {
    /// 直接使用 TCP 连接对端地址，不读取任何转发 Header。
    #[default]
    Peer,
    /// 从一个只包含裸 IP 的 Header 读取客户端地址。
    Header,
    /// 从右向左跳过可信代理，取第一个不可信地址。
    ForwardedChain,
}

/// 部署级 RUM 客户端 IP 识别配置，持久化在 `instance_settings` 单例中。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct ClientIpResolverSettings {
    pub mode: ClientIpMode,
    pub header_name: String,
    pub trusted_proxy_cidrs: Vec<String>,
    pub fallback_to_peer: bool,
    /// 是否允许代理 Header 把私网、loopback 或链路本地地址解析为客户端 IP。
    pub allow_private_client_ips: bool,
    pub max_chain_length: u16,
}

impl Default for ClientIpResolverSettings {
    fn default() -> Self {
        Self {
            mode: ClientIpMode::Peer,
            header_name: String::new(),
            trusted_proxy_cidrs: Vec::new(),
            fallback_to_peer: true,
            allow_private_client_ips: false,
            max_chain_length: 16,
        }
    }
}

/// 实例级（全局，非 per-org）设置：自助注册策略、数据面来源与入口安全策略。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSettings {
    /// 是否开放自助注册（公开 signup 接口）。
    pub signup_enabled: bool,
    /// 注册是否需审批：true → 新用户 pending、Owner/Admin 通过后激活；false → 注册即 active。
    pub signup_require_approval: bool,
    /// 服务图数据来源模式：`intake`（各进程内存配对 + flush，低延迟）或 `storage`
    /// （单例 worker 从存储重算，跨节点正确）。默认 `intake`。
    pub service_graph_source: String,
    /// 跨集群联邦：本集群稳定唯一 id（事件 source/writer，联邦内唯一、跨重启稳定）。
    /// 非空即启用联邦；留空 = 关闭（不发不收、零开销）。
    pub federation_cluster_id: String,
    /// 联邦 outbox drain → 推送远端的周期（秒）。
    pub federation_drain_interval_secs: i64,
    /// 联邦单次推送批量上限。
    pub federation_push_batch_size: i64,
    /// 联邦接收端去重表保留窗口（秒）。
    pub federation_seen_events_ttl_secs: i64,
    /// 联邦集群拓扑 gossip 周期（秒）。
    pub federation_gossip_interval_secs: i64,
    /// RUM 入口如何从可信连接来源识别客户端 IP。
    #[serde(default)]
    pub rum_client_ip_resolver: ClientIpResolverSettings,
    /// 审计记录保留天数（组织审计与平台审计共用）：更早的记录每天清理一次；0 = 永久保留。
    #[serde(default = "default_audit_retention_days")]
    pub audit_retention_days: u32,
    /// 数据流没有设置保留天数时保留的天数：compactor 与分区清单维护每一轮读一次。
    #[serde(default = "default_stream_retention_days")]
    pub default_retention_days: u32,
    /// 拨测结果保留天数：更早的结果、它们的附件与已结束的任务每天清理一次；0 = 永久保留。
    #[serde(default = "default_synthetic_result_retention_days")]
    pub synthetic_result_retention_days: u32,
    /// Prometheus 写入：一个组织同时保持活跃的 series 数上限。
    #[serde(default = "default_max_active_series_per_org")]
    pub max_active_series_per_org: u32,
    /// Prometheus 写入：一个组织里一个指标同时保持活跃的 series 数上限，不超过组织的上限。
    #[serde(default = "default_max_active_series_per_metric")]
    pub max_active_series_per_metric: u32,
    /// The most distinct values APM records an hour.
    #[serde(default)]
    pub apm_limits: crate::domain::apm::ApmLimits,
    /// 分角色集群里，告警规则没设评估延迟时往回挪多少秒（0 到 3600）：查询只读已刷写的文件。
    #[serde(default = "default_alert_evaluation_delay_secs")]
    pub alert_evaluation_delay_secs: u32,
    /// 密码登录限速。
    #[serde(default)]
    pub signin_limits: SigninLimits,
    /// 接入用量的小时记录保留天数：compactor 主节点每天清理更早的记录。
    #[serde(default = "default_intake_usage_retention_days")]
    pub intake_usage_retention_days: u32,
    /// 一个节点上，一个组织每小时最多分别记下多少个服务，超出的计入「其他服务」。
    #[serde(default = "default_intake_usage_max_services_per_org")]
    pub intake_usage_max_services_per_org: u32,
    pub updated_at: TimestampMicros,
}

/// 密码登录限速的可调参数。次数为 0 时关掉这一项。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct SigninLimits {
    /// 同一来源对同一账号失败几次后，这个来源对这个账号锁定。
    pub account_source_failures: u32,
    /// 同一来源失败几次后，不管哪个账号，这个来源锁定。
    pub source_failures: u32,
    /// 一个账号所有来源累计失败几次后开始限速。
    pub account_failures: u32,
    /// 账号限速时，隔几秒核对一次密码。
    pub account_interval_secs: u32,
    /// 来源锁多久，也是计数的时间窗。
    pub lock_minutes: u32,
}

impl Default for SigninLimits {
    fn default() -> Self {
        Self {
            account_source_failures: 5,
            source_failures: 30,
            account_failures: 20,
            account_interval_secs: 60,
            lock_minutes: 15,
        }
    }
}

/// 登录限速各项的上限（与 `instance_settings` 的 CHECK 约束一致）。
pub const MAX_SIGNIN_LIMIT_FAILURES: u32 = 100_000;
pub const MAX_SIGNIN_LIMIT_INTERVAL_SECS: u32 = 600;
pub const MAX_SIGNIN_LIMIT_LOCK_MINUTES: u32 = 1440;

/// 告警规则默认评估延迟（分角色集群）：intake 默认 30 秒刷写一次，留一倍余量。
pub const DEFAULT_ALERT_EVALUATION_DELAY_SECS: u32 = 60;

fn default_alert_evaluation_delay_secs() -> u32 {
    DEFAULT_ALERT_EVALUATION_DELAY_SECS
}

/// 审计记录默认保留天数。
pub const DEFAULT_AUDIT_RETENTION_DAYS: u32 = 90;
/// 审计记录保留天数上限（与 `instance_settings` 的 CHECK 约束一致）。
pub const MAX_AUDIT_RETENTION_DAYS: u32 = 3650;

fn default_audit_retention_days() -> u32 {
    DEFAULT_AUDIT_RETENTION_DAYS
}

/// 数据流默认保留天数。
pub const DEFAULT_STREAM_RETENTION_DAYS: u32 = 30;
/// 数据流保留天数的上限（与 `instance_settings` 的 CHECK 约束、数据流设置一致）。
pub const MAX_STREAM_RETENTION_DAYS: u32 = 3650;

fn default_stream_retention_days() -> u32 {
    DEFAULT_STREAM_RETENTION_DAYS
}

/// 拨测结果默认保留天数。
pub const DEFAULT_SYNTHETIC_RESULT_RETENTION_DAYS: u32 = 30;
/// 拨测结果保留天数上限（与 `instance_settings` 的 CHECK 约束一致）。
pub const MAX_SYNTHETIC_RESULT_RETENTION_DAYS: u32 = 3650;

fn default_synthetic_result_retention_days() -> u32 {
    DEFAULT_SYNTHETIC_RESULT_RETENTION_DAYS
}

/// Prometheus 写入的活跃 series 上限的默认值：每个组织、每个指标。
pub const DEFAULT_MAX_ACTIVE_SERIES_PER_ORG: u32 = 200_000;
pub const DEFAULT_MAX_ACTIVE_SERIES_PER_METRIC: u32 = 100_000;
/// 活跃 series 上限的最大值（与 `instance_settings` 的 CHECK 约束一致）。
pub const MAX_ACTIVE_SERIES_LIMIT: u32 = 100_000_000;

fn default_max_active_series_per_org() -> u32 {
    DEFAULT_MAX_ACTIVE_SERIES_PER_ORG
}

fn default_max_active_series_per_metric() -> u32 {
    DEFAULT_MAX_ACTIVE_SERIES_PER_METRIC
}

/// 接入用量默认保留天数。
pub const DEFAULT_INTAKE_USAGE_RETENTION_DAYS: u32 = 90;
/// 接入用量保留天数的下限：一个自然月的预算周期要读得全（与 `instance_settings` 的 CHECK 约束一致）。
pub const MIN_INTAKE_USAGE_RETENTION_DAYS: u32 = 35;
/// 接入用量保留天数的上限（与 `instance_settings` 的 CHECK 约束一致）。
pub const MAX_INTAKE_USAGE_RETENTION_DAYS: u32 = 3650;
/// 一个组织每小时分别记下的服务数：默认值与上限（与 `instance_settings` 的 CHECK 约束一致）。
pub const DEFAULT_INTAKE_USAGE_MAX_SERVICES_PER_ORG: u32 = 1000;
pub const MAX_INTAKE_USAGE_MAX_SERVICES_PER_ORG: u32 = 100_000;

fn default_intake_usage_retention_days() -> u32 {
    DEFAULT_INTAKE_USAGE_RETENTION_DAYS
}

fn default_intake_usage_max_services_per_org() -> u32 {
    DEFAULT_INTAKE_USAGE_MAX_SERVICES_PER_ORG
}

/// `days` 天前的那一刻：早于它的记录可以删除。0 天（永久保留）时为 `None`。
fn retention_cutoff(days: u32, now: TimestampMicros) -> Option<TimestampMicros> {
    if days == 0 {
        return None;
    }

    let window = i64::from(days).saturating_mul(86_400 * 1_000_000);
    Some(TimestampMicros(now.0.saturating_sub(window)))
}

impl InstanceSettings {
    /// 审计记录的保留起点：早于它的记录可以删除。保留天数为 0（永久保留）时为 `None`。
    pub fn audit_retention_cutoff(&self, now: TimestampMicros) -> Option<TimestampMicros> {
        retention_cutoff(self.audit_retention_days, now)
    }

    /// 拨测结果的保留起点：早于它的结果可以删除。保留天数为 0（永久保留）时为 `None`。
    pub fn synthetic_result_retention_cutoff(
        &self,
        now: TimestampMicros,
    ) -> Option<TimestampMicros> {
        retention_cutoff(self.synthetic_result_retention_days, now)
    }

    /// 接入用量的保留起点：早于它的小时记录可以删除。
    pub fn intake_usage_retention_cutoff(&self, now: TimestampMicros) -> Option<TimestampMicros> {
        retention_cutoff(self.intake_usage_retention_days, now)
    }

    /// 服务图是否走"存储重算"模式（跨节点正确）。
    pub fn service_graph_storage_mode(&self) -> bool {
        self.service_graph_source.eq_ignore_ascii_case("storage")
    }

    /// 联邦是否启用 = cluster_id 非空（事件 source/writer 必须稳定唯一）。
    pub fn federation_enabled(&self) -> bool {
        !self.federation_cluster_id.trim().is_empty()
    }
}

#[async_trait]
pub trait InstanceSettingsRepository: Send + Sync {
    /// 读取实例设置（单例；不存在时实现方返回默认值）。
    async fn get(&self) -> Result<InstanceSettings>;
    async fn update(&self, s: InstanceSettings) -> Result<InstanceSettings>;
}

#[async_trait]
pub trait OrganizationRepository: Send + Sync {
    async fn create(&self, org: Organization) -> Result<Organization>;
    async fn get(&self, id: &Id) -> Result<Organization>;
    async fn get_by_slug(&self, slug: &str) -> Result<Organization>;
    async fn list(&self) -> Result<Vec<Organization>>;
    /// Update the only mutable organization identity field.
    ///
    /// `id` and `slug` are deliberately absent so callers cannot accidentally
    /// turn either stable identifier into an editable attribute.
    async fn update_name(&self, id: &Id, name: String) -> Result<Organization>;
    /// Enable or disable a tenant organization. Implementations must reject
    /// disabling `_sys` and the final enabled tenant organization.
    async fn set_disabled(&self, id: &Id, disabled: bool) -> Result<Organization>;
    async fn delete(&self, id: &Id) -> Result<()>;
}

#[async_trait]
pub trait IamMembershipRepository: Send + Sync {
    /// Create/update the membership and replace its organization-wide user
    /// role bindings with the supplied database role ids.
    async fn upsert(&self, membership: IamMembership, role_ids: &[Id], actor_id: &Id)
    -> Result<()>;
    async fn list_for_user(&self, user_id: &Id) -> Result<Vec<IamMembership>>;
    async fn list_for_org(&self, org_id: &Id) -> Result<Vec<IamMembership>>;
    async fn assigned_roles(&self, user_id: &Id, org_id: &Id) -> Result<Vec<IamAssignedRole>>;
    /// Resolve organization-wide roles for a bounded set of users in one call.
    async fn assigned_roles_for_users(
        &self,
        user_ids: &[Id],
        org_id: &Id,
    ) -> Result<Vec<(Id, Vec<IamAssignedRole>)>> {
        let mut roles = Vec::with_capacity(user_ids.len());

        for user_id in user_ids {
            roles.push((user_id.clone(), self.assigned_roles(user_id, org_id).await?));
        }

        Ok(roles)
    }
    async fn role_id_for_purpose(&self, org_id: &Id, purpose: &str) -> Result<Id>;
    async fn remove(&self, user_id: &Id, org_id: &Id) -> Result<()>;
}

#[async_trait]
pub trait IamPlatformAdministratorRepository: Send + Sync {
    async fn list(&self) -> Result<Vec<IamPlatformAdministrator>>;
    async fn is_active(&self, user_id: &Id) -> Result<bool>;
    /// Reconcile the configured root as the one and only active platform
    /// administrator. Historical inactive assignments remain for auditability.
    async fn bootstrap_root(&self, user_id: &Id) -> Result<bool>;
}

#[async_trait]
pub trait TeamRepository: Send + Sync {
    async fn create(&self, team: Team) -> Result<Team>;
    async fn update(&self, team: Team) -> Result<Team>;
    async fn get(&self, id: &Id) -> Result<Team>;
    async fn list(&self, org_id: &Id) -> Result<Vec<Team>>;
    async fn delete(&self, id: &Id) -> Result<()>;

    /// 一个人所在的团队。数据库实现按 `member_ids` 的 GIN 索引查。
    async fn list_for_member(&self, org_id: &Id, user_id: &Id) -> Result<Vec<Team>> {
        Ok(self
            .list(org_id)
            .await?
            .into_iter()
            .filter(|team| team.member_ids.contains(user_id))
            .collect())
    }
}

/// 邮箱域准入判定。
///
/// `allowed` 是 org 配置的允许域名单：
/// - **空名单 = 不限制**（返回 `true`），保持默认行为向后兼容；
/// - 取 `email` 中最后一个 `@` 之后的部分作域名（小写）；无 `@` / 空域名 → 拒绝；
/// - 命中规则：域名等于某条允许项，或为其子域（`sub.example.com` 命中 `example.com`）。
///   允许项做小写化并剥掉前导 `@` / `.`，空项跳过。
pub fn email_domain_allowed(email: &str, allowed: &[String]) -> bool {
    if allowed.is_empty() {
        return true;
    }

    let Some(domain) = email
        .rsplit('@')
        .next()
        .map(|d| d.trim().to_ascii_lowercase())
    else {
        return false;
    };

    // rsplit 对无 '@' 的串会返回整串本身——显式排掉没有 '@' 或域名为空的情况。
    if domain.is_empty() || !email.contains('@') {
        return false;
    }

    allowed.iter().any(|raw| {
        let pat = raw
            .trim()
            .trim_start_matches('@')
            .trim_start_matches('.')
            .to_ascii_lowercase();

        !pat.is_empty() && (domain == pat || domain.ends_with(&format!(".{pat}")))
    })
}

#[cfg(test)]
mod tests {
    use super::{
        ClientIpResolverSettings, DEFAULT_AUDIT_RETENTION_DAYS, InstanceSettings, Organization,
        SYSTEM_ORG_NAME, SYSTEM_ORG_SLUG, email_domain_allowed,
    };
    use crate::shared::{ids::Id, time::TimestampMicros};

    fn instance_settings(audit_retention_days: u32) -> InstanceSettings {
        InstanceSettings {
            signup_enabled: false,
            signup_require_approval: true,
            service_graph_source: "intake".into(),
            federation_cluster_id: String::new(),
            federation_drain_interval_secs: 10,
            federation_push_batch_size: 100,
            federation_seen_events_ttl_secs: 604_800,
            federation_gossip_interval_secs: 60,
            rum_client_ip_resolver: ClientIpResolverSettings::default(),
            audit_retention_days,
            default_retention_days: super::DEFAULT_STREAM_RETENTION_DAYS,
            synthetic_result_retention_days: super::DEFAULT_SYNTHETIC_RESULT_RETENTION_DAYS,
            max_active_series_per_org: super::DEFAULT_MAX_ACTIVE_SERIES_PER_ORG,
            max_active_series_per_metric: super::DEFAULT_MAX_ACTIVE_SERIES_PER_METRIC,
            apm_limits: crate::domain::apm::ApmLimits::default(),
            alert_evaluation_delay_secs: 60,
            intake_usage_retention_days: 90,
            intake_usage_max_services_per_org: 1000,
            signin_limits: super::SigninLimits::default(),
            updated_at: TimestampMicros(0),
        }
    }

    #[test]
    fn audit_retention_cutoff_is_the_number_of_days_back_and_none_keeps_forever() {
        let day = 86_400 * 1_000_000;
        let now = TimestampMicros(1_000 * day);

        assert_eq!(
            instance_settings(90).audit_retention_cutoff(now),
            Some(TimestampMicros(910 * day))
        );
        assert_eq!(instance_settings(0).audit_retention_cutoff(now), None);
    }

    #[test]
    fn settings_stored_before_audit_retention_existed_default_to_ninety_days() {
        let mut stored = serde_json::to_value(instance_settings(30)).unwrap();

        stored.as_object_mut().unwrap().remove("auditRetentionDays");

        let settings: InstanceSettings = serde_json::from_value(stored).unwrap();
        assert_eq!(settings.audit_retention_days, DEFAULT_AUDIT_RETENTION_DAYS);
    }

    fn organization(name: &str, slug: &str, system: bool) -> Organization {
        Organization {
            id: Id::new(),
            name: name.into(),
            slug: slug.into(),
            system,
            disabled: false,
            created_at: TimestampMicros::now(),
        }
    }

    #[test]
    fn only_the_exact_system_organization_can_use_reserved_values() {
        assert!(
            organization(SYSTEM_ORG_NAME, SYSTEM_ORG_SLUG, true)
                .validate_system_invariants()
                .is_ok()
        );
        assert!(
            organization("tampered", SYSTEM_ORG_SLUG, true)
                .validate_system_invariants()
                .is_err()
        );
        assert!(
            organization(SYSTEM_ORG_NAME, "tenant", false)
                .validate_system_invariants()
                .is_err()
        );
        assert!(
            organization("tenant", SYSTEM_ORG_SLUG, false)
                .validate_system_invariants()
                .is_err()
        );
    }

    #[test]
    fn system_organization_is_never_mutable() {
        assert!(
            organization(SYSTEM_ORG_NAME, SYSTEM_ORG_SLUG, true)
                .ensure_mutable()
                .is_err()
        );
        assert!(
            organization("Tenant", "tenant", false)
                .ensure_mutable()
                .is_ok()
        );
    }

    #[test]
    fn disabled_tenant_cannot_be_used_and_system_cannot_be_disabled() {
        let mut tenant = organization("Tenant", "tenant", false);
        tenant.disabled = true;
        assert!(tenant.ensure_enabled().is_err());

        let mut system = organization(SYSTEM_ORG_NAME, SYSTEM_ORG_SLUG, true);
        system.disabled = true;
        assert!(system.validate_system_invariants().is_err());
    }

    #[test]
    fn empty_allowlist_permits_everything() {
        assert!(email_domain_allowed("anyone@whatever.io", &[]));
    }

    #[test]
    fn exact_and_subdomain_match_case_insensitive() {
        let allowed = vec!["Example.com".to_string()];
        assert!(email_domain_allowed("alice@example.com", &allowed));
        assert!(email_domain_allowed("BOB@EXAMPLE.COM", &allowed));
        // 子域命中。
        assert!(email_domain_allowed("ci@eu.example.com", &allowed));
        // 不同域拒绝，且不被后缀污染（notexample.com 不应命中 example.com）。
        assert!(!email_domain_allowed("eve@evil.com", &allowed));
        assert!(!email_domain_allowed("eve@notexample.com", &allowed));
    }

    #[test]
    fn allowlist_entries_tolerate_leading_at_or_dot() {
        let allowed = vec!["@corp.io".to_string(), ".vendor.net".to_string()];
        assert!(email_domain_allowed("dev@corp.io", &allowed));
        assert!(email_domain_allowed("ops@team.vendor.net", &allowed));
    }

    #[test]
    fn malformed_email_is_rejected_when_allowlist_set() {
        let allowed = vec!["example.com".to_string()];
        assert!(!email_domain_allowed("no-at-sign", &allowed));
        assert!(!email_domain_allowed("trailing@", &allowed));
    }
}
