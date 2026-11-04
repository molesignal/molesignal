// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 登录限制：允许登录的组，面向公网账号的身份源，以及由单点登录新建的账号能加入哪些组织。
//!
//! Google、GitHub、gitlab.com 这类身份源谁都能注册账号。这类服务必须由组织的邮箱
//! 域名或允许登录的组圈定范围，并且只认身份源确认过的邮箱，否则任何人都能开号进
//! 组织。判断看端点的主机，手工配置的通用接入同样适用。

use url::Url;

use super::provision::FederatedIdentity;
use crate::{
    api::AppState,
    domain::iam::{SsoProvider, SsoProviderConfig},
    shared::{Error, Result, ids::Id},
};

/// 谁都能注册账号的身份源主机。
const PUBLIC_HOSTS: [&str; 3] = ["accounts.google.com", "github.com", "gitlab.com"];

/// Microsoft 身份平台里不限租户的路径段：`common` / `organizations` / `consumers`，
/// 以及个人账号租户。
const MICROSOFT_OPEN_TENANTS: [&str; 4] = [
    "common",
    "organizations",
    "consumers",
    "9188040d-6c67-4c5b-b112-36a304b66dad",
];

const UNRESTRICTED: &str = "this provider accepts public accounts: add the organization's email domains, or the groups allowed to sign in, before enabling it";

const CONFINED: &str = "this account was created by another organization's single sign-on and its email has not been verified; its owner must verify the email (reset the password) before it can join";

pub(super) fn accepts_public_accounts(provider: &SsoProvider) -> bool {
    let endpoint = match &provider.config {
        SsoProviderConfig::Oidc(config) => config.issuer.as_str(),
        SsoProviderConfig::Oauth2(config) => config.authorize_url.as_str(),
        SsoProviderConfig::Saml(_) | SsoProviderConfig::Ldap(_) => return false,
    };

    let Ok(url) = Url::parse(endpoint.trim()) else {
        return false;
    };

    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();

    if host == "login.microsoftonline.com" {
        let tenant = url
            .path_segments()
            .and_then(|mut segments| segments.next())
            .unwrap_or_default()
            .to_ascii_lowercase();

        return MICROSOFT_OPEN_TENANTS.contains(&tenant.as_str());
    }

    PUBLIC_HOSTS.contains(&host.as_str())
}

/// 启用面向公网账号的服务之前，组织得配了邮箱域名，或服务限定了允许登录的组。
pub(super) async fn ensure_restricted(state: &AppState, provider: &SsoProvider) -> Result<()> {
    if !accepts_public_accounts(provider) || !provider.config.allowed_groups().is_empty() {
        return Ok(());
    }

    let domains = state.iam.email_domains.list(&provider.org_id).await?;

    if domains.is_empty() {
        return Err(Error::invalid(UNRESTRICTED));
    }

    Ok(())
}

/// 由某个服务新建、邮箱未经证明的账号只属于该服务所在的组织：别的组织加不了它，
/// 免得有人先替别的组织的人建号，等对方被加进去后再借身份源登录。本人证明邮箱后解除。
pub(crate) async fn ensure_membership_allowed(
    state: &AppState,
    user_id: &Id,
    org_id: &Id,
) -> Result<()> {
    match state.iam.sso_identities.creator_org(user_id).await? {
        Some(creator) if creator != *org_id => Err(Error::forbidden(CONFINED)),
        _ => Ok(()),
    }
}

/// 每次登录都检查：账号在允许登录的组里；面向公网账号的服务还要求邮箱已验证、
/// 范围仍有限定（邮箱域名可能在服务启用后被删光）。
pub(super) async fn ensure_identity_allowed(
    state: &AppState,
    provider: &SsoProvider,
    identity: &FederatedIdentity,
) -> Result<()> {
    let allowed_groups = provider.config.allowed_groups();

    if !allowed_groups.is_empty()
        && !identity
            .groups
            .iter()
            .any(|group| allowed_groups.contains(group))
    {
        return Err(Error::forbidden(
            "this account is not in a group allowed to sign in with this provider",
        ));
    }

    if !accepts_public_accounts(provider) {
        return Ok(());
    }

    if identity.email_verified != Some(true) {
        return Err(Error::unauthorized(
            "the identity provider has not verified this account's email address",
        ));
    }

    ensure_restricted(state, provider)
        .await
        .map_err(|_| Error::forbidden(UNRESTRICTED))
}

#[cfg(test)]
mod tests {
    use super::accepts_public_accounts;
    use crate::{
        domain::iam::{
            SsoLdapConfig, SsoOauth2Config, SsoOidcConfig, SsoPlatform, SsoProvider,
            SsoProviderConfig, SsoProviderKind,
        },
        shared::{ids::Id, time::TimestampMicros},
    };

    fn provider(config: SsoProviderConfig) -> SsoProvider {
        SsoProvider {
            id: Id::new(),
            org_id: Id::new(),
            name: "test".into(),
            kind: SsoProviderKind::Oidc,
            platform: SsoPlatform::Custom,
            enabled: true,
            config,
            created_at: TimestampMicros::now(),
            updated_at: TimestampMicros::now(),
        }
    }

    fn oidc(issuer: &str) -> SsoProvider {
        provider(SsoProviderConfig::Oidc(SsoOidcConfig {
            issuer: issuer.into(),
            ..SsoOidcConfig::default()
        }))
    }

    #[test]
    fn public_identity_sources_are_recognized_by_host() {
        assert!(accepts_public_accounts(&oidc(
            "https://accounts.google.com"
        )));
        assert!(accepts_public_accounts(&oidc("https://gitlab.com")));
        assert!(accepts_public_accounts(&oidc(
            "https://login.microsoftonline.com/common/v2.0"
        )));
        assert!(!accepts_public_accounts(&oidc(
            "https://login.microsoftonline.com/72f988bf-86f1-41af-91ab-2d7cd011db47/v2.0"
        )));
        assert!(!accepts_public_accounts(&oidc(
            "https://gitlab.example.com"
        )));
        assert!(!accepts_public_accounts(&oidc(
            "https://sso.example.com/realms/ops"
        )));

        let github = provider(SsoProviderConfig::Oauth2(SsoOauth2Config {
            authorize_url: "https://github.com/login/oauth/authorize".into(),
            ..SsoOauth2Config::default()
        }));
        assert!(accepts_public_accounts(&github));

        let enterprise = provider(SsoProviderConfig::Oauth2(SsoOauth2Config {
            authorize_url: "https://github.example.com/login/oauth/authorize".into(),
            ..SsoOauth2Config::default()
        }));
        assert!(!accepts_public_accounts(&enterprise));

        let directory = provider(SsoProviderConfig::Ldap(SsoLdapConfig::default()));
        assert!(!accepts_public_accounts(&directory));
    }
}
