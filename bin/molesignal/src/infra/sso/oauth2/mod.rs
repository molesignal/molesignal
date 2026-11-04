// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! OAuth 2.0 授权码流程。
//!
//! [`OAuthClient`] 是 OIDC 与 OAuth 2.0 共用的客户端：拼授权地址、用授权码换令牌、
//! 带访问令牌读 JSON 接口。[`OAuth2LoginFlow`] 是不签发 ID Token 的 OAuth 2.0 登录：
//! 通用接入读用户信息接口并按字段映射取身份，GitHub 与飞书按平台读法取身份。

mod feishu;
mod github;

use std::{collections::BTreeMap, time::Duration};

use reqwest::{Client, header};
use serde_json::Value;
use url::Url;

use super::claims::{ExternalIdentity, map_identity};
use crate::{
    domain::iam::{SsoOauth2Config, SsoPlatform},
    shared::{
        Error, Result,
        http_trace::{self, HttpTarget},
    },
};

/// GitHub 的 API 拒绝没有 User-Agent 的请求。
const USER_AGENT: &str = concat!("MoleSignal/", env!("CARGO_PKG_VERSION"));

/// 换令牌请求体的编码：标准是表单，飞书只收 JSON。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenRequestEncoding {
    Form,
    Json,
}

#[derive(Debug, Clone)]
pub struct OAuthClientConfig {
    pub authorize_url: String,
    pub token_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct OAuthTokens {
    pub access_token: String,
    pub id_token: Option<String>,
}

pub struct OAuthClient {
    config: OAuthClientConfig,
    encoding: TokenRequestEncoding,
    http: Client,
}

impl OAuthClient {
    pub fn new(config: OAuthClientConfig, encoding: TokenRequestEncoding) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(USER_AGENT)
            .build()
            .map_err(|e| Error::internal(format!("oauth http client: {e}")))?;

        Ok(Self {
            config,
            encoding,
            http,
        })
    }

    /// 浏览器要去的授权地址。`extra` 是协议附加参数，如 OIDC 的 `nonce`。
    pub fn authorization_url(&self, state: &str, extra: &[(&str, &str)]) -> Result<Url> {
        let mut url = Url::parse(&self.config.authorize_url)
            .map_err(|e| Error::invalid(format!("authorize_url: {e}")))?;

        {
            let mut query = url.query_pairs_mut();

            query
                .append_pair("response_type", "code")
                .append_pair("client_id", &self.config.client_id)
                .append_pair("redirect_uri", &self.config.redirect_uri);

            if !self.config.scopes.is_empty() {
                query.append_pair("scope", &self.config.scopes.join(" "));
            }

            query.append_pair("state", state);

            for (name, value) in extra {
                query.append_pair(name, value);
            }
        }

        Ok(url)
    }

    /// 用授权码换令牌。GitHub 失败时也回 200，所以以有没有 `access_token` 为准。
    pub async fn exchange_code(&self, code: &str) -> Result<OAuthTokens> {
        let fields = BTreeMap::from([
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", self.config.redirect_uri.as_str()),
            ("client_id", self.config.client_id.as_str()),
            ("client_secret", self.config.client_secret.as_str()),
        ]);

        let request = self
            .http
            .post(&self.config.token_url)
            .header(header::ACCEPT, "application/json");

        let request = match self.encoding {
            TokenRequestEncoding::Form => request.form(&fields),
            TokenRequestEncoding::Json => request.json(&fields),
        };

        let response = http_trace::send(&self.http, request, HttpTarget::ThirdParty)
            .await
            .map_err(|e| Error::internal(format!("oauth token http: {e}")))?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let value = serde_json::from_str::<Value>(&body).ok();

        let access_token = value
            .as_ref()
            .and_then(|value| value.get("access_token"))
            .and_then(Value::as_str)
            .filter(|token| !token.is_empty());

        match access_token {
            Some(access_token) if status.is_success() => Ok(OAuthTokens {
                access_token: access_token.to_owned(),
                id_token: value
                    .as_ref()
                    .and_then(|value| value.get("id_token"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            }),
            _ => Err(Error::unauthorized(format!(
                "idp token endpoint {status}: {}",
                describe_failure(value.as_ref(), &body)
            ))),
        }
    }

    /// 带访问令牌读一个 JSON 接口（用户信息、组织列表等）。
    pub async fn get_json(&self, url: &str, access_token: &str) -> Result<Value> {
        let request = self
            .http
            .get(url)
            .bearer_auth(access_token)
            .header(header::ACCEPT, "application/json");

        let response = http_trace::send(&self.http, request, HttpTarget::ThirdParty)
            .await
            .map_err(|e| Error::internal(format!("idp http {url}: {e}")))?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let value = serde_json::from_str::<Value>(&body).ok();

        if !status.is_success() {
            return Err(Error::unauthorized(format!(
                "idp {url} {status}: {}",
                describe_failure(value.as_ref(), &body)
            )));
        }

        value.ok_or_else(|| Error::unauthorized(format!("idp {url} did not return JSON")))
    }
}

/// 身份源回的错误说明：优先取 OAuth 的 `error_description` / `error`，其次是平台自己的
/// `msg` / `message`，都没有就截取正文。
fn describe_failure(value: Option<&Value>, body: &str) -> String {
    let field = |name: &str| {
        value
            .and_then(|value| value.get(name))
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
    };

    let described = ["error_description", "error", "msg", "message"]
        .into_iter()
        .find_map(field);

    match described {
        Some(text) => text.to_owned(),
        None => body.chars().take(300).collect(),
    }
}

/// 不签发 ID Token 的 OAuth 2.0 登录。
pub struct OAuth2LoginFlow {
    config: SsoOauth2Config,
    platform: SsoPlatform,
    client: OAuthClient,
}

impl OAuth2LoginFlow {
    pub fn new(config: &SsoOauth2Config, platform: SsoPlatform) -> Result<Self> {
        let encoding = match platform {
            SsoPlatform::Feishu => TokenRequestEncoding::Json,
            _ => TokenRequestEncoding::Form,
        };

        let client = OAuthClient::new(
            OAuthClientConfig {
                authorize_url: config.authorize_url.clone(),
                token_url: config.token_url.clone(),
                client_id: config.client_id.clone(),
                client_secret: config.client_secret.clone(),
                redirect_uri: config.redirect_uri.clone(),
                scopes: config.scopes.clone(),
            },
            encoding,
        )?;

        Ok(Self {
            config: config.clone(),
            platform,
            client,
        })
    }

    pub fn authorization_url(&self, state: &str) -> Result<Url> {
        self.client.authorization_url(state, &[])
    }

    pub async fn identity(&self, code: &str) -> Result<ExternalIdentity> {
        let tokens = self.client.exchange_code(code).await?;
        let userinfo_url = self.config.userinfo_url.as_str();

        match self.platform {
            SsoPlatform::Github => {
                github::identity(&self.client, userinfo_url, &tokens.access_token).await
            }
            SsoPlatform::Feishu => {
                let user = self
                    .client
                    .get_json(userinfo_url, &tokens.access_token)
                    .await?;

                feishu::identity(&user, &self.config.field_mapping)
            }
            _ => {
                let user = self
                    .client
                    .get_json(userinfo_url, &tokens.access_token)
                    .await?;

                map_identity(&user, &self.config.field_mapping)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn client() -> OAuthClient {
        OAuthClient::new(
            OAuthClientConfig {
                authorize_url: "https://idp.example/authorize?audience=api".into(),
                token_url: "https://idp.example/token".into(),
                client_id: "molesignal".into(),
                client_secret: "shh".into(),
                redirect_uri: "https://obs.example/api/v1/auth/sso/callback".into(),
                scopes: vec!["openid".into(), "email".into()],
            },
            TokenRequestEncoding::Form,
        )
        .expect("client")
    }

    #[test]
    fn authorization_url_keeps_existing_query_and_adds_protocol_parameters() {
        let url = client()
            .authorization_url("st-1", &[("nonce", "n-1")])
            .expect("authorization url");
        let query: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query.get("audience").map(String::as_str), Some("api"));
        assert_eq!(query.get("response_type").map(String::as_str), Some("code"));
        assert_eq!(
            query.get("client_id").map(String::as_str),
            Some("molesignal")
        );
        assert_eq!(query.get("scope").map(String::as_str), Some("openid email"));
        assert_eq!(query.get("state").map(String::as_str), Some("st-1"));
        assert_eq!(query.get("nonce").map(String::as_str), Some("n-1"));
    }

    #[test]
    fn failures_prefer_oauth_descriptions_then_platform_messages() {
        let oauth = json!({ "error": "bad_verification_code", "error_description": "The code passed is incorrect or expired." });
        assert_eq!(
            describe_failure(Some(&oauth), ""),
            "The code passed is incorrect or expired."
        );

        let feishu = json!({ "code": 20003, "msg": "invalid code" });
        assert_eq!(describe_failure(Some(&feishu), ""), "invalid code");
        assert_eq!(describe_failure(None, "plain text"), "plain text");
    }
}
