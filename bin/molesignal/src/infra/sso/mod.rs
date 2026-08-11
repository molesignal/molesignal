// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SSO 登录流。
//!
//! - [`oidc::OidcLoginFlow`]：手写 OIDC Authorization Code flow，用 reqwest 直跑
//!   IdP 的 `/authorize` + `/token` 端点，ID Token JWT 用 `jsonwebtoken` 解
//!   （生产可以加入 JWKS 缓存；当前只对 alg=RS256 + 静态公钥 / HS256 + secret 走）。
//! - [`saml::SamlLoginFlow`]：SAML SP redirect + signed assertion 验证。
//! - [`ldap::LdapLoginFlow`]：LDAPS / StartTLS 用户查找与 simple bind。
//! - [`session_repo`]：sso_sessions 表 CRUD（issued_at + last_login_at）。

pub mod jwks;
pub mod ldap;
pub mod oidc;
pub mod saml;
pub mod session_repo;
pub mod state_store;
pub mod xmldsig;

pub use jwks::{Jwk, JwkSet, JwksCache, verify_rs256_id_token};
pub use ldap::{LdapConfig, LdapLoginFlow, LdapUser};
pub use oidc::{OidcConfig, OidcLoginFlow};
pub use saml::{SamlAssertion, SamlConfig, SamlLoginFlow};
pub use session_repo::{PgSsoSessionRepository, SsoSession, SsoSessionRepository};
pub use state_store::{SsoStateEntry, SsoStateStore};
