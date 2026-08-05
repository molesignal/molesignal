// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SSO provider domain model, identity-field mapping, and role mapping.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Result, ids::Id, time::TimestampMicros};

/// SSO 接入类型 — 决定运行时实例化的认证 flow。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SsoProviderKind {
    Oidc,
    Saml,
    Ldap,
}

/// MoleSignal 身份字段到认证平台字段的映射。
///
/// OIDC 值是 claim 名或点分路径；SAML 值是 Attribute Name（`NameID` 是特殊值）；
/// LDAP 值是属性名（`dn` 是特殊值）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsoFieldMapping {
    pub subject: String,
    pub email: String,
    pub display_name: String,
    pub groups: String,
}

impl SsoFieldMapping {
    pub fn oidc() -> Self {
        Self {
            subject: "sub".into(),
            email: "email".into(),
            display_name: "name".into(),
            groups: "groups".into(),
        }
    }

    pub fn saml() -> Self {
        Self {
            subject: "NameID".into(),
            email: "email".into(),
            display_name: "name".into(),
            groups: "groups".into(),
        }
    }

    pub fn ldap() -> Self {
        Self {
            subject: "dn".into(),
            email: "mail".into(),
            display_name: "displayName".into(),
            groups: "memberOf".into(),
        }
    }
}

impl Default for SsoFieldMapping {
    fn default() -> Self {
        Self::oidc()
    }
}

/// Provider-specific 配置。写入和读取时都必须由 [`SsoProviderKind`] 引导
/// variant，不能只按 JSON shape 猜测。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SsoProviderConfig {
    Oidc(SsoOidcConfig),
    Saml(SsoSamlConfig),
    Ldap(SsoLdapConfig),
}

impl SsoProviderConfig {
    pub fn group_role_mapping(&self) -> &BTreeMap<String, String> {
        match self {
            Self::Oidc(config) => &config.group_role_mapping,
            Self::Saml(config) => &config.group_role_mapping,
            Self::Ldap(config) => &config.group_role_mapping,
        }
    }

    pub fn default_role_id(&self) -> Option<&str> {
        match self {
            Self::Oidc(config) => config.default_role_id.as_deref(),
            Self::Saml(config) => config.default_role_id.as_deref(),
            Self::Ldap(config) => config.default_role_id.as_deref(),
        }
    }

    pub fn referenced_role_ids(&self) -> BTreeSet<&str> {
        self.group_role_mapping()
            .values()
            .map(String::as_str)
            .chain(self.default_role_id())
            .collect()
    }

    pub fn references_role(&self, role_id: &Id) -> bool {
        self.referenced_role_ids().contains(role_id.0.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SsoOidcConfig {
    pub issuer: String,
    pub authorize_url: String,
    pub token_url: String,
    #[serde(default)]
    pub userinfo_url: Option<String>,
    #[serde(default)]
    pub discovery_url: Option<String>,
    #[serde(default)]
    pub jwks_uri: Option<String>,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub field_mapping: SsoFieldMapping,
    #[serde(default)]
    pub group_role_mapping: BTreeMap<String, String>,
    #[serde(default)]
    pub default_role_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsoSamlConfig {
    pub sp_entity_id: String,
    pub idp_entity_id: String,
    pub idp_sso_url: String,
    pub idp_x509_cert: String,
    pub assertion_consumer_url: String,
    #[serde(default = "SsoFieldMapping::saml")]
    pub field_mapping: SsoFieldMapping,
    #[serde(default)]
    pub group_role_mapping: BTreeMap<String, String>,
    #[serde(default)]
    pub default_role_id: Option<String>,
}

impl Default for SsoSamlConfig {
    fn default() -> Self {
        Self {
            sp_entity_id: String::new(),
            idp_entity_id: String::new(),
            idp_sso_url: String::new(),
            idp_x509_cert: String::new(),
            assertion_consumer_url: String::new(),
            field_mapping: SsoFieldMapping::saml(),
            group_role_mapping: BTreeMap::new(),
            default_role_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsoLdapConfig {
    /// `ldaps://`，或搭配 `start_tls = true` 的 `ldap://` URL。
    pub url: String,
    #[serde(default)]
    pub start_tls: bool,
    /// 用于查找用户 DN 的只读服务账号；两者都为空时使用匿名搜索。
    #[serde(default)]
    pub bind_dn: String,
    #[serde(default)]
    pub bind_password: String,
    pub base_dn: String,
    /// 必须包含 `{username}`；运行时会先做 LDAP filter escaping 再替换。
    pub user_filter: String,
    #[serde(default = "SsoFieldMapping::ldap")]
    pub field_mapping: SsoFieldMapping,
    #[serde(default)]
    pub group_role_mapping: BTreeMap<String, String>,
    #[serde(default)]
    pub default_role_id: Option<String>,
}

impl Default for SsoLdapConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            start_tls: false,
            bind_dn: String::new(),
            bind_password: String::new(),
            base_dn: String::new(),
            user_filter: "(&(objectClass=person)(|(mail={username})(uid={username})))".into(),
            field_mapping: SsoFieldMapping::ldap(),
            group_role_mapping: BTreeMap::new(),
            default_role_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsoProvider {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub kind: SsoProviderKind,
    pub enabled: bool,
    pub config: SsoProviderConfig,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[async_trait]
pub trait SsoProviderRepository: Send + Sync {
    async fn create(&self, provider: SsoProvider) -> Result<SsoProvider>;
    async fn update(&self, provider: SsoProvider) -> Result<SsoProvider>;
    async fn get(&self, id: &Id) -> Result<SsoProvider>;
    async fn list(&self, org_id: &Id) -> Result<Vec<SsoProvider>>;
    async fn list_enabled(&self, org_id: &Id) -> Result<Vec<SsoProvider>>;
    /// 全表（跨 org）列出 enabled 的某 kind 的 provider；专给未登录态的
    /// `/auth/sso/login` 路径用 — caller 在这里没有 `org_id`。
    async fn list_enabled_by_kind(&self, kind: SsoProviderKind) -> Result<Vec<SsoProvider>>;
    async fn set_enabled(&self, id: &Id, enabled: bool) -> Result<SsoProvider>;
    async fn delete(&self, id: &Id) -> Result<()>;
}
