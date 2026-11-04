// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SSO 登录流。
//!
//! - [`oidc::OidcLoginFlow`]：OIDC Authorization Code flow，ID Token 按 JWKS
//!   （RSA / EC / EdDSA）或 Client Secret（HMAC）验签，可再合并 UserInfo；
//!   [`oidc::discovery`] 给管理端读 `.well-known/openid-configuration`。
//! - [`oauth2::OAuth2LoginFlow`]：不签发 ID Token 的 OAuth 2.0，身份取自用户信息接口；
//!   GitHub 与飞书按平台读法取身份。
//! - [`saml::SamlLoginFlow`]：SAML SP redirect + signed assertion 验证；
//!   [`saml::metadata`] 解析 IdP 元数据。
//! - [`ldap::LdapLoginFlow`]：LDAPS / StartTLS 用户查找与 simple bind。
//! - [`claims`]：按身份字段映射从 JSON 声明里取身份。
//! - [`identity`]：外部身份与账号的绑定（只按绑定登录），以及已有账号首次关联时等待本人
//!   确认的请求。
//! - [`state_store`]：登录发起的 `state` / `nonce` 与登录完成后的一次性交接凭据。
//! - [`signin_tests`]：测试登录的记录，和作废旧结果用的配置摘要。

pub mod claims;
pub mod identity;
pub mod ldap;
pub mod oauth2;
pub mod oidc;
pub mod saml;
pub mod signin_tests;
pub mod state_store;
pub mod xmldsig;

pub use claims::ExternalIdentity;
pub use identity::{
    PgSsoIdentityRepository, SsoIdentity, SsoIdentityProof, SsoIdentityRepository, SsoIdentityView,
    link_requests::{
        LinkedIdentity, PgSsoLinkRequestRepository, SsoLinkRequest, SsoLinkRequestRepository,
    },
};
pub use ldap::{LdapConfig, LdapLoginFlow, LdapUser};
pub use oauth2::OAuth2LoginFlow;
pub use oidc::{OidcLoginFlow, jwks::JwksCache};
pub use saml::{SamlAssertion, SamlConfig, SamlLoginFlow};
pub use signin_tests::{
    PgSsoSigninTestRepository, SsoSigninTest, SsoSigninTestReport, SsoSigninTestRepository,
    SsoSigninTestStatus, SsoSigninTestSummary, TestedAccount, TestedFailure, TestedIdentity,
    TestedMapping, TestedRole, TestedRoleSource, config_hash,
};
pub use state_store::{SsoHandoff, SsoStateEntry, SsoStateStore};
