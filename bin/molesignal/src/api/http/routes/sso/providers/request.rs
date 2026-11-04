// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SSO provider 的请求体与校验：按类型取对应的配置块，检查必填项、地址、平台与类型
//! 是否相配、字段与角色映射、允许登录的组；没填的密钥沿用已保存的值。

use std::collections::BTreeMap;

use serde::Deserialize;
use url::Url;

use super::mapping::{normalize_allowed_groups, normalize_field_mapping, normalize_role_mapping};
use crate::{
    domain::iam::{
        SsoFieldMapping, SsoLdapConfig, SsoOauth2Config, SsoOidcConfig, SsoPlatform,
        SsoProviderConfig, SsoProviderKind, SsoSamlConfig,
    },
    infra::sso::{LdapConfig, LdapLoginFlow},
    shared::{Error, Result},
};

#[derive(Debug, Deserialize)]
pub(super) struct UpsertRequest {
    name: String,
    kind: SsoProviderKind,
    #[serde(default)]
    platform: SsoPlatform,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    oidc: Option<OidcConfigDto>,
    #[serde(default)]
    oauth2: Option<Oauth2ConfigDto>,
    #[serde(default)]
    saml: Option<SamlConfigDto>,
    #[serde(default)]
    ldap: Option<LdapConfigDto>,
}

/// 角色映射与登录限制，每种类型都有。
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccessDto {
    #[serde(default)]
    group_role_mapping: BTreeMap<String, String>,
    #[serde(default)]
    default_role_id: Option<String>,
    #[serde(default)]
    allowed_groups: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    #[serde(flatten)]
    access: AccessDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Oauth2ConfigDto {
    #[serde(default)]
    authorize_url: String,
    #[serde(default)]
    token_url: String,
    #[serde(default)]
    userinfo_url: String,
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    client_secret: Option<String>,
    #[serde(default)]
    redirect_uri: String,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default = "SsoFieldMapping::oauth2")]
    field_mapping: SsoFieldMapping,
    #[serde(flatten)]
    access: AccessDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    #[serde(flatten)]
    access: AccessDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    #[serde(flatten)]
    access: AccessDto,
}

/// 校验后的服务定义。
#[derive(Debug)]
pub(super) struct ValidatedProvider {
    pub(super) name: String,
    pub(super) kind: SsoProviderKind,
    pub(super) platform: SsoPlatform,
    pub(super) enabled: Option<bool>,
    pub(super) config: SsoProviderConfig,
}

/// 校验后的角色映射与登录限制。
struct Access {
    group_role_mapping: BTreeMap<String, String>,
    default_role_id: Option<String>,
    allowed_groups: Vec<String>,
}

impl AccessDto {
    fn normalize(self) -> Result<Access> {
        let (group_role_mapping, default_role_id) =
            normalize_role_mapping(self.group_role_mapping, self.default_role_id)?;

        Ok(Access {
            group_role_mapping,
            default_role_id,
            allowed_groups: normalize_allowed_groups(self.allowed_groups)?,
        })
    }
}

pub(super) fn build_config(
    request: UpsertRequest,
    existing: Option<&SsoProviderConfig>,
) -> Result<ValidatedProvider> {
    let UpsertRequest {
        name,
        kind,
        platform,
        enabled,
        oidc,
        oauth2,
        saml,
        ldap,
    } = request;

    let name = name.trim();

    if name.is_empty() || name.len() > 128 {
        return Err(Error::invalid("name must be 1..128 characters"));
    }

    if !platform.supports(kind) {
        return Err(Error::invalid(format!(
            "the {} platform does not sign in with {}",
            platform.as_str(),
            kind.as_str()
        )));
    }

    let config = match kind {
        SsoProviderKind::Oidc => {
            let dto =
                oidc.ok_or_else(|| Error::invalid("kind = oidc requires `oidc` config block"))?;

            SsoProviderConfig::Oidc(oidc_config(dto, existing)?)
        }
        SsoProviderKind::Oauth2 => {
            let dto = oauth2
                .ok_or_else(|| Error::invalid("kind = oauth2 requires `oauth2` config block"))?;

            SsoProviderConfig::Oauth2(oauth2_config(dto, existing)?)
        }
        SsoProviderKind::Saml => {
            let dto =
                saml.ok_or_else(|| Error::invalid("kind = saml requires `saml` config block"))?;

            SsoProviderConfig::Saml(saml_config(dto)?)
        }
        SsoProviderKind::Ldap => {
            let dto =
                ldap.ok_or_else(|| Error::invalid("kind = ldap requires `ldap` config block"))?;

            SsoProviderConfig::Ldap(ldap_config(dto, existing)?)
        }
    };

    Ok(ValidatedProvider {
        name: name.to_owned(),
        kind,
        platform,
        enabled,
        config,
    })
}

fn oidc_config(dto: OidcConfigDto, existing: Option<&SsoProviderConfig>) -> Result<SsoOidcConfig> {
    let field_mapping = normalize_field_mapping(dto.field_mapping, SsoProviderKind::Oidc)?;
    let access = dto.access.normalize()?;

    let existing_secret = match existing {
        Some(SsoProviderConfig::Oidc(config)) => Some(config.client_secret.as_str()),
        _ => None,
    };

    Ok(SsoOidcConfig {
        issuer: endpoint("oidc.issuer", &dto.issuer)?,
        authorize_url: endpoint("oidc.authorize_url", &dto.authorize_url)?,
        token_url: endpoint("oidc.token_url", &dto.token_url)?,
        userinfo_url: optional_endpoint("oidc.userinfo_url", dto.userinfo_url)?,
        jwks_uri: optional_endpoint("oidc.jwks_uri", dto.jwks_uri)?,
        client_id: required("oidc.client_id", &dto.client_id)?,
        client_secret: secret_or_existing(dto.client_secret, existing_secret),
        redirect_uri: endpoint("oidc.redirect_uri", &dto.redirect_uri)?,
        scopes: scopes(dto.scopes),
        field_mapping,
        group_role_mapping: access.group_role_mapping,
        default_role_id: access.default_role_id,
        allowed_groups: access.allowed_groups,
    })
}

fn oauth2_config(
    dto: Oauth2ConfigDto,
    existing: Option<&SsoProviderConfig>,
) -> Result<SsoOauth2Config> {
    let field_mapping = normalize_field_mapping(dto.field_mapping, SsoProviderKind::Oauth2)?;
    let access = dto.access.normalize()?;

    let existing_secret = match existing {
        Some(SsoProviderConfig::Oauth2(config)) => Some(config.client_secret.as_str()),
        _ => None,
    };

    Ok(SsoOauth2Config {
        authorize_url: endpoint("oauth2.authorize_url", &dto.authorize_url)?,
        token_url: endpoint("oauth2.token_url", &dto.token_url)?,
        userinfo_url: endpoint("oauth2.userinfo_url", &dto.userinfo_url)?,
        client_id: required("oauth2.client_id", &dto.client_id)?,
        client_secret: secret_or_existing(dto.client_secret, existing_secret),
        redirect_uri: endpoint("oauth2.redirect_uri", &dto.redirect_uri)?,
        scopes: scopes(dto.scopes),
        field_mapping,
        group_role_mapping: access.group_role_mapping,
        default_role_id: access.default_role_id,
        allowed_groups: access.allowed_groups,
    })
}

fn saml_config(dto: SamlConfigDto) -> Result<SsoSamlConfig> {
    let field_mapping = normalize_field_mapping(dto.field_mapping, SsoProviderKind::Saml)?;
    let access = dto.access.normalize()?;

    Ok(SsoSamlConfig {
        sp_entity_id: required("saml.sp_entity_id", &dto.sp_entity_id)?,
        idp_entity_id: required("saml.idp_entity_id", &dto.idp_entity_id)?,
        idp_sso_url: endpoint("saml.idp_sso_url", &dto.idp_sso_url)?,
        idp_x509_cert: required("saml.idp_x509_cert", &dto.idp_x509_cert)?,
        assertion_consumer_url: endpoint(
            "saml.assertion_consumer_url",
            &dto.assertion_consumer_url,
        )?,
        field_mapping,
        group_role_mapping: access.group_role_mapping,
        default_role_id: access.default_role_id,
        allowed_groups: access.allowed_groups,
    })
}

fn ldap_config(dto: LdapConfigDto, existing: Option<&SsoProviderConfig>) -> Result<SsoLdapConfig> {
    let bind_password = if dto.bind_dn.trim().is_empty() {
        String::new()
    } else {
        let existing_password = match existing {
            Some(SsoProviderConfig::Ldap(config)) => Some(config.bind_password.as_str()),
            _ => None,
        };

        secret_or_existing(dto.bind_password, existing_password)
    };

    let field_mapping = normalize_field_mapping(dto.field_mapping, SsoProviderKind::Ldap)?;
    let access = dto.access.normalize()?;

    let config = SsoLdapConfig {
        url: dto.url,
        start_tls: dto.start_tls,
        bind_dn: dto.bind_dn,
        bind_password,
        base_dn: dto.base_dn,
        user_filter: dto.user_filter,
        field_mapping,
        group_role_mapping: access.group_role_mapping,
        default_role_id: access.default_role_id,
        allowed_groups: access.allowed_groups,
    };

    LdapLoginFlow::new(ldap_flow_config(&config))?;
    Ok(config)
}

fn required(field: &str, value: &str) -> Result<String> {
    let value = value.trim();

    if value.is_empty() {
        return Err(Error::invalid(format!("{field} is required")));
    }

    Ok(value.to_owned())
}

/// 必填的 http(s) 地址。
fn endpoint(field: &str, value: &str) -> Result<String> {
    let value = required(field, value)?;

    let valid = Url::parse(&value)
        .is_ok_and(|url| matches!(url.scheme(), "https" | "http") && url.host_str().is_some());

    if !valid {
        return Err(Error::invalid(format!("{field} must be an http(s) URL")));
    }

    Ok(value)
}

fn optional_endpoint(field: &str, value: Option<String>) -> Result<Option<String>> {
    match value.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some(value) => endpoint(field, value).map(Some),
    }
}

fn scopes(scopes: Vec<String>) -> Vec<String> {
    scopes
        .iter()
        .flat_map(|scope| scope.split_whitespace())
        .map(str::to_owned)
        .collect()
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
    use crate::domain::iam::{SsoPlatform, SsoProviderConfig};

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
                "bindDn": "cn=reader,dc=example,dc=com",
                "baseDn": "dc=example,dc=com",
                "userFilter": "(mail={username})"
            }
        }))
        .expect("deserialize LDAP provider request");
        let validated = build_config(request, Some(&existing)).expect("validate LDAP config");
        let SsoProviderConfig::Ldap(config) = validated.config else {
            panic!("expected LDAP config");
        };
        assert_eq!(config.bind_password, "existing-secret");
    }

    #[test]
    fn github_is_an_oauth2_platform_with_allowed_groups() {
        let request: UpsertRequest = serde_json::from_value(serde_json::json!({
            "name": "GitHub",
            "kind": "oauth2",
            "platform": "github",
            "oauth2": {
                "authorizeUrl": "https://github.com/login/oauth/authorize",
                "tokenUrl": "https://github.com/login/oauth/access_token",
                "userinfoUrl": "https://api.github.com/user",
                "clientId": "Iv1.abc",
                "clientSecret": "secret",
                "redirectUri": "https://obs.example.com/api/v1/auth/sso/callback",
                "scopes": ["read:user user:email", "read:org"],
                "allowedGroups": [" acme ", "acme", ""]
            }
        }))
        .expect("deserialize GitHub provider request");
        let validated = build_config(request, None).expect("validate GitHub config");
        assert_eq!(validated.platform, SsoPlatform::Github);
        let SsoProviderConfig::Oauth2(config) = validated.config else {
            panic!("expected OAuth 2.0 config");
        };
        assert_eq!(config.scopes, ["read:user", "user:email", "read:org"]);
        assert_eq!(config.allowed_groups, ["acme"]);
        assert_eq!(config.field_mapping.subject, "id");
    }

    #[test]
    fn platforms_and_endpoints_are_checked() {
        let oidc_github: UpsertRequest = serde_json::from_value(serde_json::json!({
            "name": "GitHub",
            "kind": "oidc",
            "platform": "github",
            "oidc": {}
        }))
        .expect("deserialize");
        assert!(build_config(oidc_github, None).is_err());

        let not_a_url: UpsertRequest = serde_json::from_value(serde_json::json!({
            "name": "Keycloak",
            "kind": "oidc",
            "platform": "keycloak",
            "oidc": {
                "issuer": "sso.example.com/realms/ops",
                "authorizeUrl": "https://sso.example.com/realms/ops/protocol/openid-connect/auth",
                "tokenUrl": "https://sso.example.com/realms/ops/protocol/openid-connect/token",
                "clientId": "molesignal",
                "redirectUri": "https://obs.example.com/api/v1/auth/sso/callback"
            }
        }))
        .expect("deserialize");
        let error = build_config(not_a_url, None).expect_err("issuer must be a URL");
        assert!(error.to_string().contains("oidc.issuer"));
    }
}
