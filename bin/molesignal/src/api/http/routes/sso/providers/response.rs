// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SSO provider 的响应体：管理端看到的完整配置（密钥只回是否已设置），以及登录页
//! 公开的服务列表。

use std::collections::BTreeMap;

use serde::Serialize;

use super::super::signin_test::TestStanding;
use crate::domain::iam::{
    SsoFieldMapping, SsoLdapConfig, SsoOauth2Config, SsoOidcConfig, SsoPlatform, SsoProvider,
    SsoProviderConfig, SsoProviderKind, SsoSamlConfig,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProviderResponse {
    id: String,
    org_id: String,
    name: String,
    kind: SsoProviderKind,
    platform: SsoPlatform,
    enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    oidc: Option<OidcConfigResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    oauth2: Option<Oauth2ConfigResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    saml: Option<SamlConfigResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ldap: Option<LdapConfigResponse>,
    /// 当前配置的测试情况；读取时才有。
    #[serde(skip_serializing_if = "Option::is_none")]
    test: Option<TestStanding>,
    created_at: i64,
    updated_at: i64,
}

impl ProviderResponse {
    pub(super) fn with_test(mut self, test: Option<TestStanding>) -> Self {
        self.test = test;
        self
    }
}

impl From<SsoProvider> for ProviderResponse {
    fn from(provider: SsoProvider) -> Self {
        let mut response = Self {
            id: provider.id.0,
            org_id: provider.org_id.0,
            name: provider.name,
            kind: provider.kind,
            platform: provider.platform,
            enabled: provider.enabled,
            oidc: None,
            oauth2: None,
            saml: None,
            ldap: None,
            test: None,
            created_at: provider.created_at.0,
            updated_at: provider.updated_at.0,
        };

        match provider.config {
            SsoProviderConfig::Oidc(config) => response.oidc = Some(config.into()),
            SsoProviderConfig::Oauth2(config) => response.oauth2 = Some(config.into()),
            SsoProviderConfig::Saml(config) => response.saml = Some(config.into()),
            SsoProviderConfig::Ldap(config) => response.ldap = Some(config.into()),
        }

        response
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OidcConfigResponse {
    issuer: String,
    authorize_url: String,
    token_url: String,
    userinfo_url: Option<String>,
    jwks_uri: Option<String>,
    client_id: String,
    has_client_secret: bool,
    redirect_uri: String,
    scopes: Vec<String>,
    field_mapping: SsoFieldMapping,
    group_role_mapping: BTreeMap<String, String>,
    default_role_id: Option<String>,
    allowed_groups: Vec<String>,
}

impl From<SsoOidcConfig> for OidcConfigResponse {
    fn from(config: SsoOidcConfig) -> Self {
        Self {
            issuer: config.issuer,
            authorize_url: config.authorize_url,
            token_url: config.token_url,
            userinfo_url: config.userinfo_url,
            jwks_uri: config.jwks_uri,
            client_id: config.client_id,
            has_client_secret: !config.client_secret.is_empty(),
            redirect_uri: config.redirect_uri,
            scopes: config.scopes,
            field_mapping: config.field_mapping,
            group_role_mapping: config.group_role_mapping,
            default_role_id: config.default_role_id,
            allowed_groups: config.allowed_groups,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Oauth2ConfigResponse {
    authorize_url: String,
    token_url: String,
    userinfo_url: String,
    client_id: String,
    has_client_secret: bool,
    redirect_uri: String,
    scopes: Vec<String>,
    field_mapping: SsoFieldMapping,
    group_role_mapping: BTreeMap<String, String>,
    default_role_id: Option<String>,
    allowed_groups: Vec<String>,
}

impl From<SsoOauth2Config> for Oauth2ConfigResponse {
    fn from(config: SsoOauth2Config) -> Self {
        Self {
            authorize_url: config.authorize_url,
            token_url: config.token_url,
            userinfo_url: config.userinfo_url,
            client_id: config.client_id,
            has_client_secret: !config.client_secret.is_empty(),
            redirect_uri: config.redirect_uri,
            scopes: config.scopes,
            field_mapping: config.field_mapping,
            group_role_mapping: config.group_role_mapping,
            default_role_id: config.default_role_id,
            allowed_groups: config.allowed_groups,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SamlConfigResponse {
    sp_entity_id: String,
    idp_entity_id: String,
    idp_sso_url: String,
    idp_x509_cert: String,
    assertion_consumer_url: String,
    field_mapping: SsoFieldMapping,
    group_role_mapping: BTreeMap<String, String>,
    default_role_id: Option<String>,
    allowed_groups: Vec<String>,
}

impl From<SsoSamlConfig> for SamlConfigResponse {
    fn from(config: SsoSamlConfig) -> Self {
        Self {
            sp_entity_id: config.sp_entity_id,
            idp_entity_id: config.idp_entity_id,
            idp_sso_url: config.idp_sso_url,
            idp_x509_cert: config.idp_x509_cert,
            assertion_consumer_url: config.assertion_consumer_url,
            field_mapping: config.field_mapping,
            group_role_mapping: config.group_role_mapping,
            default_role_id: config.default_role_id,
            allowed_groups: config.allowed_groups,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LdapConfigResponse {
    url: String,
    start_tls: bool,
    bind_dn: String,
    has_bind_password: bool,
    base_dn: String,
    user_filter: String,
    field_mapping: SsoFieldMapping,
    group_role_mapping: BTreeMap<String, String>,
    default_role_id: Option<String>,
    allowed_groups: Vec<String>,
}

impl From<SsoLdapConfig> for LdapConfigResponse {
    fn from(config: SsoLdapConfig) -> Self {
        Self {
            url: config.url,
            start_tls: config.start_tls,
            bind_dn: config.bind_dn,
            has_bind_password: !config.bind_password.is_empty(),
            base_dn: config.base_dn,
            user_filter: config.user_filter,
            field_mapping: config.field_mapping,
            group_role_mapping: config.group_role_mapping,
            default_role_id: config.default_role_id,
            allowed_groups: config.allowed_groups,
        }
    }
}

/// 登录页按 `platform` 画图标，按 `kind` 决定跳转还是提交账号密码。
#[derive(Debug, Serialize)]
pub(crate) struct PublicProviderResponse {
    id: String,
    pub(super) name: String,
    kind: SsoProviderKind,
    platform: SsoPlatform,
}

impl From<SsoProvider> for PublicProviderResponse {
    fn from(provider: SsoProvider) -> Self {
        Self {
            id: provider.id.0,
            name: provider.name,
            kind: provider.kind,
            platform: provider.platform,
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct AssignableRoleResponse {
    id: String,
    name: String,
}

impl AssignableRoleResponse {
    pub(super) fn new(id: String, name: String) -> Self {
        Self { id, name }
    }
}
