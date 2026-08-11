// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SSO provider HTTP DTOs, secret redaction, and input validation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::mapping::{normalize_field_mapping, normalize_role_mapping};
use crate::{
    domain::iam::{
        SsoFieldMapping, SsoLdapConfig, SsoOidcConfig, SsoProvider, SsoProviderConfig,
        SsoProviderKind, SsoSamlConfig,
    },
    infra::sso::{LdapConfig, LdapLoginFlow},
    shared::{Error, Result},
};

#[derive(Debug, Serialize)]
pub(super) struct ProviderResponse {
    id: String,
    org_id: String,
    name: String,
    kind: SsoProviderKind,
    enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    oidc: Option<OidcConfigResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    saml: Option<SamlConfigResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ldap: Option<LdapConfigResponse>,
    created_at_micros: i64,
    updated_at_micros: i64,
}

impl From<SsoProvider> for ProviderResponse {
    fn from(provider: SsoProvider) -> Self {
        let (oidc, saml, ldap) = match provider.config {
            SsoProviderConfig::Oidc(config) => (Some(OidcConfigResponse::from(config)), None, None),
            SsoProviderConfig::Saml(config) => (None, Some(SamlConfigResponse::from(config)), None),
            SsoProviderConfig::Ldap(config) => (None, None, Some(LdapConfigResponse::from(config))),
        };
        Self {
            id: provider.id.0,
            org_id: provider.org_id.0,
            name: provider.name,
            kind: provider.kind,
            enabled: provider.enabled,
            oidc,
            saml,
            ldap,
            created_at_micros: provider.created_at.0,
            updated_at_micros: provider.updated_at.0,
        }
    }
}

#[derive(Debug, Serialize)]
struct OidcConfigResponse {
    issuer: String,
    authorize_url: String,
    token_url: String,
    userinfo_url: Option<String>,
    discovery_url: Option<String>,
    jwks_uri: Option<String>,
    client_id: String,
    has_client_secret: bool,
    redirect_uri: String,
    scopes: Vec<String>,
    field_mapping: SsoFieldMapping,
    group_role_mapping: BTreeMap<String, String>,
    default_role_id: Option<String>,
}

impl From<SsoOidcConfig> for OidcConfigResponse {
    fn from(config: SsoOidcConfig) -> Self {
        Self {
            issuer: config.issuer,
            authorize_url: config.authorize_url,
            token_url: config.token_url,
            userinfo_url: config.userinfo_url,
            discovery_url: config.discovery_url,
            jwks_uri: config.jwks_uri,
            client_id: config.client_id,
            has_client_secret: !config.client_secret.is_empty(),
            redirect_uri: config.redirect_uri,
            scopes: config.scopes,
            field_mapping: config.field_mapping,
            group_role_mapping: config.group_role_mapping,
            default_role_id: config.default_role_id,
        }
    }
}

#[derive(Debug, Serialize)]
struct SamlConfigResponse {
    sp_entity_id: String,
    idp_entity_id: String,
    idp_sso_url: String,
    idp_x509_cert: String,
    assertion_consumer_url: String,
    field_mapping: SsoFieldMapping,
    group_role_mapping: BTreeMap<String, String>,
    default_role_id: Option<String>,
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
        }
    }
}

#[derive(Debug, Serialize)]
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
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct PublicProviderResponse {
    id: String,
    pub(super) name: String,
    kind: SsoProviderKind,
}

impl From<SsoProvider> for PublicProviderResponse {
    fn from(provider: SsoProvider) -> Self {
        Self {
            id: provider.id.0,
            name: provider.name,
            kind: provider.kind,
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

#[derive(Debug, Deserialize)]
pub(super) struct UpsertRequest {
    name: String,
    kind: SsoProviderKind,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    oidc: Option<OidcConfigDto>,
    #[serde(default)]
    saml: Option<SamlConfigDto>,
    #[serde(default)]
    ldap: Option<LdapConfigDto>,
}

#[derive(Debug, Deserialize)]
struct OidcConfigDto {
    #[serde(default)]
    issuer: String,
    #[serde(default)]
    authorize_url: String,
    #[serde(default)]
    token_url: String,
    #[serde(default)]
    userinfo_url: Option<String>,
    #[serde(default)]
    discovery_url: Option<String>,
    #[serde(default)]
    jwks_uri: Option<String>,
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    client_secret: Option<String>,
    #[serde(default)]
    redirect_uri: String,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    field_mapping: SsoFieldMapping,
    #[serde(default)]
    group_role_mapping: BTreeMap<String, String>,
    #[serde(default)]
    default_role_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SamlConfigDto {
    #[serde(default)]
    sp_entity_id: String,
    #[serde(default)]
    idp_entity_id: String,
    #[serde(default)]
    idp_sso_url: String,
    #[serde(default)]
    idp_x509_cert: String,
    #[serde(default)]
    assertion_consumer_url: String,
    #[serde(default = "SsoFieldMapping::saml")]
    field_mapping: SsoFieldMapping,
    #[serde(default)]
    group_role_mapping: BTreeMap<String, String>,
    #[serde(default)]
    default_role_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LdapConfigDto {
    #[serde(default)]
    url: String,
    #[serde(default)]
    start_tls: bool,
    #[serde(default)]
    bind_dn: String,
    #[serde(default)]
    bind_password: Option<String>,
    #[serde(default)]
    base_dn: String,
    #[serde(default)]
    user_filter: String,
    #[serde(default = "SsoFieldMapping::ldap")]
    field_mapping: SsoFieldMapping,
    #[serde(default)]
    group_role_mapping: BTreeMap<String, String>,
    #[serde(default)]
    default_role_id: Option<String>,
}

pub(super) fn build_config(
    request: UpsertRequest,
    existing: Option<&SsoProviderConfig>,
) -> Result<(String, SsoProviderKind, Option<bool>, SsoProviderConfig)> {
    let UpsertRequest {
        name,
        kind,
        enabled,
        oidc,
        saml,
        ldap,
    } = request;
    let name = name.trim();
    if name.is_empty() || name.len() > 128 {
        return Err(Error::invalid("name must be 1..128 characters"));
    }
    let config = match kind {
        SsoProviderKind::Oidc => {
            let dto =
                oidc.ok_or_else(|| Error::invalid("kind = oidc requires `oidc` config block"))?;
            if dto.issuer.is_empty()
                || dto.authorize_url.is_empty()
                || dto.token_url.is_empty()
                || dto.client_id.is_empty()
                || dto.redirect_uri.is_empty()
            {
                return Err(Error::invalid(
                    "oidc.issuer/authorize_url/token_url/client_id/redirect_uri are required",
                ));
            }
            let field_mapping = normalize_field_mapping(dto.field_mapping, SsoProviderKind::Oidc)?;
            let (group_role_mapping, default_role_id) =
                normalize_role_mapping(dto.group_role_mapping, dto.default_role_id)?;
            SsoProviderConfig::Oidc(SsoOidcConfig {
                issuer: dto.issuer,
                authorize_url: dto.authorize_url,
                token_url: dto.token_url,
                userinfo_url: dto.userinfo_url,
                discovery_url: dto.discovery_url,
                jwks_uri: dto.jwks_uri,
                client_id: dto.client_id,
                client_secret: secret_or_existing(
                    dto.client_secret,
                    match existing {
                        Some(SsoProviderConfig::Oidc(config)) => {
                            Some(config.client_secret.as_str())
                        }
                        _ => None,
                    },
                ),
                redirect_uri: dto.redirect_uri,
                scopes: dto.scopes,
                field_mapping,
                group_role_mapping,
                default_role_id,
            })
        }
        SsoProviderKind::Saml => {
            let dto =
                saml.ok_or_else(|| Error::invalid("kind = saml requires `saml` config block"))?;
            if dto.sp_entity_id.is_empty()
                || dto.idp_entity_id.is_empty()
                || dto.idp_sso_url.is_empty()
                || dto.idp_x509_cert.is_empty()
                || dto.assertion_consumer_url.is_empty()
            {
                return Err(Error::invalid(
                    "saml.sp_entity_id/idp_entity_id/idp_sso_url/idp_x509_cert/assertion_consumer_url are required",
                ));
            }
            let field_mapping = normalize_field_mapping(dto.field_mapping, SsoProviderKind::Saml)?;
            let (group_role_mapping, default_role_id) =
                normalize_role_mapping(dto.group_role_mapping, dto.default_role_id)?;
            SsoProviderConfig::Saml(SsoSamlConfig {
                sp_entity_id: dto.sp_entity_id,
                idp_entity_id: dto.idp_entity_id,
                idp_sso_url: dto.idp_sso_url,
                idp_x509_cert: dto.idp_x509_cert,
                assertion_consumer_url: dto.assertion_consumer_url,
                field_mapping,
                group_role_mapping,
                default_role_id,
            })
        }
        SsoProviderKind::Ldap => {
            let dto =
                ldap.ok_or_else(|| Error::invalid("kind = ldap requires `ldap` config block"))?;
            let bind_password = if dto.bind_dn.trim().is_empty() {
                String::new()
            } else {
                secret_or_existing(
                    dto.bind_password,
                    match existing {
                        Some(SsoProviderConfig::Ldap(config)) => {
                            Some(config.bind_password.as_str())
                        }
                        _ => None,
                    },
                )
            };
            let field_mapping = normalize_field_mapping(dto.field_mapping, SsoProviderKind::Ldap)?;
            let (group_role_mapping, default_role_id) =
                normalize_role_mapping(dto.group_role_mapping, dto.default_role_id)?;
            let config = SsoLdapConfig {
                url: dto.url,
                start_tls: dto.start_tls,
                bind_dn: dto.bind_dn,
                bind_password,
                base_dn: dto.base_dn,
                user_filter: dto.user_filter,
                field_mapping,
                group_role_mapping,
                default_role_id,
            };
            LdapLoginFlow::new(ldap_flow_config(&config))?;
            SsoProviderConfig::Ldap(config)
        }
    };
    Ok((name.to_owned(), kind, enabled, config))
}

fn secret_or_existing(candidate: Option<String>, existing: Option<&str>) -> String {
    candidate
        .filter(|secret| !secret.is_empty())
        .or_else(|| existing.map(str::to_owned))
        .unwrap_or_default()
}

fn ldap_flow_config(config: &SsoLdapConfig) -> LdapConfig {
    LdapConfig {
        url: config.url.clone(),
        start_tls: config.start_tls,
        bind_dn: config.bind_dn.clone(),
        bind_password: config.bind_password.clone(),
        base_dn: config.base_dn.clone(),
        user_filter: config.user_filter.clone(),
        field_mapping: config.field_mapping.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{UpsertRequest, build_config};
    use crate::domain::iam::SsoProviderConfig;

    #[test]
    fn ldap_update_preserves_omitted_bind_password() {
        let existing = SsoProviderConfig::Ldap(crate::domain::iam::SsoLdapConfig {
            url: "ldaps://ldap.example.com".into(),
            bind_dn: "cn=reader,dc=example,dc=com".into(),
            bind_password: "existing-secret".into(),
            base_dn: "dc=example,dc=com".into(),
            ..Default::default()
        });
        let request: UpsertRequest = serde_json::from_value(serde_json::json!({
            "name": "Directory",
            "kind": "ldap",
            "ldap": {
                "url": "ldaps://ldap.example.com",
                "bind_dn": "cn=reader,dc=example,dc=com",
                "base_dn": "dc=example,dc=com",
                "user_filter": "(mail={username})"
            }
        }))
        .expect("deserialize LDAP provider request");
        let (_, _, _, config) =
            build_config(request, Some(&existing)).expect("validate LDAP config");
        let SsoProviderConfig::Ldap(config) = config else {
            panic!("expected LDAP config");
        };
        assert_eq!(config.bind_password, "existing-secret");
    }
}
