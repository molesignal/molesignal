// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! OIDC Authorization Code flow。
//!
//! 授权地址带 `nonce`；换回的 ID Token 按 [`id_token`] 验签和校验声明；配了 UserInfo
//! 端点时再读一次，补上 ID Token 里没有的 claim（如 GitLab 只在 UserInfo 里给的
//! `groups`）；最后按字段映射取身份。

pub mod discovery;
pub mod id_token;
pub mod jwks;

use serde_json::Value;
use url::Url;

use self::{id_token::Expected, jwks::JwksCache};
use super::{
    claims::{ExternalIdentity, map_identity},
    oauth2::{OAuthClient, OAuthClientConfig, TokenRequestEncoding},
};
use crate::{
    domain::iam::SsoOidcConfig,
    shared::{Error, Result},
};

/// 没配 Scopes 时请求的默认值。
const DEFAULT_SCOPES: [&str; 3] = ["openid", "profile", "email"];

pub struct OidcLoginFlow {
    config: SsoOidcConfig,
    client: OAuthClient,
}

impl OidcLoginFlow {
    pub fn new(config: &SsoOidcConfig) -> Result<Self> {
        let scopes = if config.scopes.is_empty() {
            DEFAULT_SCOPES.map(str::to_owned).to_vec()
        } else {
            config.scopes.clone()
        };

        let client = OAuthClient::new(
            OAuthClientConfig {
                authorize_url: config.authorize_url.clone(),
                token_url: config.token_url.clone(),
                client_id: config.client_id.clone(),
                client_secret: config.client_secret.clone(),
                redirect_uri: config.redirect_uri.clone(),
                scopes,
            },
            TokenRequestEncoding::Form,
        )?;

        Ok(Self {
            config: config.clone(),
            client,
        })
    }

    /// 构造 `GET /authorize?...` 302 目标。`state` / `nonce` 由 handler 生成并保管。
    pub fn authorization_url(&self, state: &str, nonce: &str) -> Result<Url> {
        self.client.authorization_url(state, &[("nonce", nonce)])
    }

    /// 用授权码换令牌，验过 ID Token 后取身份。
    pub async fn identity(
        &self,
        code: &str,
        jwks_cache: &JwksCache,
        nonce: &str,
    ) -> Result<ExternalIdentity> {
        let tokens = self.client.exchange_code(code).await?;

        let id_token = tokens
            .id_token
            .as_deref()
            .ok_or_else(|| Error::unauthorized("idp returned no id_token"))?;

        let jwks_uri = self
            .config
            .jwks_uri
            .as_deref()
            .map(str::trim)
            .filter(|uri| !uri.is_empty());

        let expected = Expected {
            issuer: &self.config.issuer,
            audience: &self.config.client_id,
            nonce,
            client_secret: &self.config.client_secret,
        };

        let mut claims =
            id_token::verify(id_token, &expected, jwks_uri.map(|uri| (jwks_cache, uri))).await?;

        let userinfo_url = self
            .config
            .userinfo_url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty());

        if let Some(url) = userinfo_url {
            let userinfo = self.client.get_json(url, &tokens.access_token).await?;
            merge_userinfo(&mut claims, userinfo)?;
        }

        map_identity(&claims, &self.config.field_mapping)
    }
}

/// UserInfo 的 `sub` 必须与 ID Token 一致（OIDC Core 5.3.2）；其余 claim 只补
/// ID Token 里没有的，签过名的值优先。
fn merge_userinfo(claims: &mut Value, userinfo: Value) -> Result<()> {
    let (Value::Object(claims), Value::Object(userinfo)) = (claims, userinfo) else {
        return Err(Error::unauthorized(
            "id_token or userinfo is not a JSON object",
        ));
    };

    if let Some(subject) = userinfo.get("sub")
        && claims.get("sub") != Some(subject)
    {
        return Err(Error::unauthorized(
            "userinfo sub does not match the id_token sub",
        ));
    }

    for (name, value) in userinfo {
        claims.entry(name).or_insert(value);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn userinfo_fills_missing_claims_without_overriding_signed_ones() {
        let mut claims = json!({ "sub": "u-1", "email": "signed@example.com" });
        let userinfo = json!({ "sub": "u-1", "email": "other@example.com", "groups": ["acme"] });
        merge_userinfo(&mut claims, userinfo).expect("merged");
        assert_eq!(claims["email"], "signed@example.com");
        assert_eq!(claims["groups"], json!(["acme"]));
    }

    #[test]
    fn userinfo_for_another_subject_is_rejected() {
        let mut claims = json!({ "sub": "u-1" });
        assert!(merge_userinfo(&mut claims, json!({ "sub": "u-2" })).is_err());
    }

    #[test]
    fn authorization_url_requests_default_scopes_and_nonce() {
        let flow = OidcLoginFlow::new(&SsoOidcConfig {
            issuer: "https://idp.example".into(),
            authorize_url: "https://idp.example/auth".into(),
            token_url: "https://idp.example/token".into(),
            client_id: "molesignal".into(),
            redirect_uri: "https://obs.example/api/v1/auth/sso/callback".into(),
            ..SsoOidcConfig::default()
        })
        .expect("flow");
        let url = flow.authorization_url("st-1", "n-1").expect("url");
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(
            query.get("scope").map(String::as_str),
            Some("openid profile email")
        );
        assert_eq!(query.get("nonce").map(String::as_str), Some("n-1"));
    }
}
