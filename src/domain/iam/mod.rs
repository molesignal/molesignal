// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! IAM：身份、组织上下文、成员关系、角色与权限。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Result, ids::Id, time::TimestampMicros};

pub mod access;
pub mod api_token;
pub mod catalog;
pub mod navigation;
mod sso;

pub use molesignal_permission_macro::{permission, resource_permission};
pub use sso::*;

pub const SYSTEM_ORG_NAME: &str = "_sys";
pub const SYSTEM_ORG_SLUG: &str = "_sys";
/// Stable business purpose whose concrete `_sys` role is selected by
/// `iam_builtin_role_purposes`. This is deliberately not a role key or name.
pub const PLATFORM_ADMINISTRATOR_ROLE_PURPOSE: &str = "platform_administrator";

#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct Team {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub member_ids: Vec<Id>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[serde(default, deny_unknown_fields)]
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
pub struct InstanceSettings {
    /// 是否开放自助注册（公开 signup 接口）。
    pub signup_enabled: bool,
    /// 注册是否需审批：true → 新用户 pending、Owner/Admin 通过后激活；false → 注册即 active。
    pub signup_require_approval: bool,
    /// 服务图数据来源模式：`ingest`（各进程内存配对 + flush，低延迟）或 `storage`
    /// （单例 worker 从存储重算，跨节点正确）。默认 `ingest`。
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
    pub updated_at: TimestampMicros,
}

impl InstanceSettings {
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
    use super::{Organization, SYSTEM_ORG_NAME, SYSTEM_ORG_SLUG, email_domain_allowed};
    use crate::shared::{ids::Id, time::TimestampMicros};

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
