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
    /// 不签发 ID Token 的 OAuth 2.0 授权码流程：身份取自用户信息接口。
    Oauth2,
    Saml,
    Ldap,
}

impl SsoProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Oidc => "oidc",
            Self::Oauth2 => "oauth2",
            Self::Saml => "saml",
            Self::Ldap => "ldap",
        }
    }
}

/// 预置的身份平台：决定登录页上的图标和管理端的配置模板，GitHub 与飞书的
/// OAuth 2.0 登录还按平台读取身份。`Custom` 是手工配置的通用协议接入。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SsoPlatform {
    #[default]
    Custom,
    Google,
    Microsoft,
    Github,
    Gitlab,
    Okta,
    Auth0,
    Keycloak,
    Authentik,
    Feishu,
    Adfs,
    Jumpcloud,
    ActiveDirectory,
    Openldap,
    Freeipa,
}

impl SsoPlatform {
    pub const ALL: [Self; 15] = [
        Self::Custom,
        Self::Google,
        Self::Microsoft,
        Self::Github,
        Self::Gitlab,
        Self::Okta,
        Self::Auth0,
        Self::Keycloak,
        Self::Authentik,
        Self::Feishu,
        Self::Adfs,
        Self::Jumpcloud,
        Self::ActiveDirectory,
        Self::Openldap,
        Self::Freeipa,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Custom => "custom",
            Self::Google => "google",
            Self::Microsoft => "microsoft",
            Self::Github => "github",
            Self::Gitlab => "gitlab",
            Self::Okta => "okta",
            Self::Auth0 => "auth0",
            Self::Keycloak => "keycloak",
            Self::Authentik => "authentik",
            Self::Feishu => "feishu",
            Self::Adfs => "adfs",
            Self::Jumpcloud => "jumpcloud",
            Self::ActiveDirectory => "active_directory",
            Self::Openldap => "openldap",
            Self::Freeipa => "freeipa",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|platform| platform.as_str() == value)
    }

    /// 平台能用哪种接入类型；通用接入不限。
    pub fn supports(self, kind: SsoProviderKind) -> bool {
        use SsoProviderKind::{Ldap, Oauth2, Oidc, Saml};

        match self {
            Self::Custom => true,
            Self::Google | Self::Microsoft | Self::Okta => matches!(kind, Oidc | Saml),
            Self::Gitlab | Self::Auth0 | Self::Keycloak | Self::Authentik => kind == Oidc,
            Self::Github | Self::Feishu => kind == Oauth2,
            Self::Adfs | Self::Jumpcloud => kind == Saml,
            Self::ActiveDirectory | Self::Openldap | Self::Freeipa => kind == Ldap,
        }
    }
}

/// MoleSignal 身份字段到认证平台字段的映射。
///
/// OIDC 与 OAuth 2.0 的值是 claim 名或点分路径，可用 `|` 列出几个来源，依次取第一个
/// 有值的；SAML 值是 Attribute Name（`NameID` 是特殊值）；LDAP 值是属性名（`dn` 是
/// 特殊值）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

    pub fn oauth2() -> Self {
        Self {
            subject: "id".into(),
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
    Oauth2(SsoOauth2Config),
    Saml(SsoSamlConfig),
    Ldap(SsoLdapConfig),
}

impl SsoProviderConfig {
    pub fn group_role_mapping(&self) -> &BTreeMap<String, String> {
        match self {
            Self::Oidc(config) => &config.group_role_mapping,
            Self::Oauth2(config) => &config.group_role_mapping,
            Self::Saml(config) => &config.group_role_mapping,
            Self::Ldap(config) => &config.group_role_mapping,
        }
    }

    pub fn default_role_id(&self) -> Option<&str> {
        match self {
            Self::Oidc(config) => config.default_role_id.as_deref(),
            Self::Oauth2(config) => config.default_role_id.as_deref(),
            Self::Saml(config) => config.default_role_id.as_deref(),
            Self::Ldap(config) => config.default_role_id.as_deref(),
        }
    }

    /// 非空时只有属于其中任一组的账号能通过该服务登录。
    pub fn allowed_groups(&self) -> &[String] {
        match self {
            Self::Oidc(config) => &config.allowed_groups,
            Self::Oauth2(config) => &config.allowed_groups,
            Self::Saml(config) => &config.allowed_groups,
            Self::Ldap(config) => &config.allowed_groups,
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
#[serde(rename_all = "camelCase")]
pub struct SsoOidcConfig {
    pub issuer: String,
    pub authorize_url: String,
    pub token_url: String,
    #[serde(default)]
    pub userinfo_url: Option<String>,
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
    #[serde(default)]
    pub allowed_groups: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SsoOauth2Config {
    pub authorize_url: String,
    pub token_url: String,
    /// 用户信息接口；GitHub 去掉末尾的 `/user` 得到 API 根地址。
    pub userinfo_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default = "SsoFieldMapping::oauth2")]
    pub field_mapping: SsoFieldMapping,
    #[serde(default)]
    pub group_role_mapping: BTreeMap<String, String>,
    #[serde(default)]
    pub default_role_id: Option<String>,
    #[serde(default)]
    pub allowed_groups: Vec<String>,
}

impl Default for SsoOauth2Config {
    fn default() -> Self {
        Self {
            authorize_url: String::new(),
            token_url: String::new(),
            userinfo_url: String::new(),
            client_id: String::new(),
            client_secret: String::new(),
            redirect_uri: String::new(),
            scopes: Vec::new(),
            field_mapping: SsoFieldMapping::oauth2(),
            group_role_mapping: BTreeMap::new(),
            default_role_id: None,
            allowed_groups: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    #[serde(default)]
    pub allowed_groups: Vec<String>,
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
            allowed_groups: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    #[serde(default)]
    pub allowed_groups: Vec<String>,
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
            allowed_groups: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SsoProvider {
    pub id: Id,
    pub org_id: Id,
    pub name: String,
    pub kind: SsoProviderKind,
    pub platform: SsoPlatform,
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

#[cfg(test)]
mod tests {
    use super::{SsoPlatform, SsoProviderKind};

    #[test]
    fn platform_names_round_trip() {
        for platform in SsoPlatform::ALL {
            assert_eq!(SsoPlatform::parse(platform.as_str()), Some(platform));

            let json = serde_json::to_value(platform).expect("serialize platform");
            assert_eq!(json, serde_json::Value::from(platform.as_str()));
        }

        assert_eq!(SsoPlatform::parse("wecom"), None);
    }

    #[test]
    fn presets_are_bound_to_their_protocols() {
        assert!(SsoPlatform::Github.supports(SsoProviderKind::Oauth2));
        assert!(!SsoPlatform::Github.supports(SsoProviderKind::Oidc));
        assert!(SsoPlatform::Microsoft.supports(SsoProviderKind::Saml));
        assert!(!SsoPlatform::Keycloak.supports(SsoProviderKind::Ldap));
        assert!(SsoPlatform::Custom.supports(SsoProviderKind::Oauth2));
    }
}
