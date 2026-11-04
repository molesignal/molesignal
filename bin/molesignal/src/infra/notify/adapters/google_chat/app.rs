// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Google Chat 应用：服务账号的密钥换访问令牌，Chat 与 Pub/Sub 的接口地址。
//!
//! 发通知（[`super::GoogleChatAppConnectorAdapter`]）与 Mole Agent 在 Google Chat 里回答
//! （`infra::im::google_chat`）都用它。令牌按范围分开缓存：发消息用 `chat.bot`，从 Pub/Sub
//! 订阅里拉消息用 `pubsub`。

use std::{
    io::Cursor,
    time::{SystemTime, UNIX_EPOCH},
};

use aws_lc_rs::{
    rand::SystemRandom,
    signature::{RSA_PKCS1_SHA256, RsaKeyPair},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rustls::pki_types::PrivateKeyDer;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    super::common::{
        TOKENS, TokenCache, client, default_timeout_secs, provider_message, validate_http_url,
        validate_timeout, with_query,
    },
    GOOGLE_CHAT_APP_CONNECTOR_TYPE,
};
use crate::shared::{
    Error, Result,
    http_trace::{self, HttpTarget},
};

pub(crate) const CHAT_BOT_SCOPE: &str = "https://www.googleapis.com/auth/chat.bot";
pub(crate) const PUBSUB_SCOPE: &str = "https://www.googleapis.com/auth/pubsub";
const JWT_BEARER: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";

pub(crate) const GOOGLE_CHAT_API_BASE_URL: &str = "https://chat.googleapis.com/v1";
/// Mole Agent 收消息的 Pub/Sub。
pub(crate) const GOOGLE_PUBSUB_BASE_URL: &str = "https://pubsub.googleapis.com/v1";

fn default_api_base_url() -> String {
    GOOGLE_CHAT_API_BASE_URL.into()
}

fn default_pubsub_base_url() -> String {
    GOOGLE_PUBSUB_BASE_URL.into()
}

/// Google 换令牌的地址：服务账号密钥里的 `token_uri` 只能是其中之一，免得签好的断言被发到别处。
/// 早年的密钥写的是第二个。
const GOOGLE_TOKEN_URIS: [&str; 2] = [
    "https://oauth2.googleapis.com/token",
    "https://accounts.google.com/o/oauth2/token",
];

fn default_token_uri() -> String {
    GOOGLE_TOKEN_URIS[0].into()
}

/// 服务账号的密钥文件（JSON）里用到的部分。
#[derive(Debug, Clone, Deserialize)]
struct ServiceAccount {
    client_email: String,
    private_key: String,
    #[serde(default = "default_token_uri")]
    token_uri: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    /// 服务账号的密钥：JSON 对象，或它的文本。
    credentials: Value,
    #[serde(default = "default_api_base_url")]
    api_base_url: String,
    /// Mole Agent 收消息的 Pub/Sub 订阅：`projects/<项目>/subscriptions/<订阅>`。
    #[serde(default)]
    subscription: String,
    #[serde(default = "default_pubsub_base_url")]
    pubsub_base_url: String,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
}

#[derive(Debug, Default, Deserialize)]
struct TokenResponse {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    expires_in: u64,
    #[serde(default)]
    error_description: String,
}

/// 一个 Chat 应用的服务账号与接口地址。
pub(crate) struct GoogleChatApp {
    config: Config,
    account: ServiceAccount,
}

impl GoogleChatApp {
    pub(crate) fn parse(value: &Value) -> Result<Self> {
        let config: Config = serde_json::from_value(value.clone())
            .map_err(|error| Error::invalid(format!("invalid google_chat_app config: {error}")))?;

        let credentials = match &config.credentials {
            Value::String(text) => serde_json::from_str(text).map_err(|_| {
                Error::invalid("google_chat_app credentials must be a service account key (JSON)")
            })?,
            other => other.clone(),
        };

        let account: ServiceAccount = serde_json::from_value(credentials).map_err(|_| {
            Error::invalid(
                "google_chat_app credentials need the client_email and private_key of a service account",
            )
        })?;

        signing_key(&account.private_key)?;
        validate_http_url(&account.token_uri, "google_chat_app token_uri")?;
        validate_http_url(&config.api_base_url, "google_chat_app apiBaseUrl")?;
        validate_http_url(&config.pubsub_base_url, "google_chat_app pubsubBaseUrl")?;
        validate_timeout(config.timeout_secs, GOOGLE_CHAT_APP_CONNECTOR_TYPE)?;

        // 不填，或是订阅的完整资源名。
        let subscription = config.subscription.trim();
        let well_formed = subscription.is_empty()
            || (subscription.starts_with("projects/") && subscription.contains("/subscriptions/"));

        if !well_formed {
            return Err(Error::invalid(
                "google_chat_app subscription must look like projects/<project>/subscriptions/<name>",
            ));
        }

        Ok(Self { config, account })
    }

    /// 密钥里的 `token_uri` 是 Google 的。保存、测试与直发前查；解析本身不查，测试里可以换成本机的服务。
    pub(crate) fn validate_token_uri(&self) -> Result<()> {
        let token_uri = self.account.token_uri.trim().trim_end_matches('/');

        if GOOGLE_TOKEN_URIS.contains(&token_uri) {
            return Ok(());
        }

        Err(Error::invalid(
            "google_chat_app credentials token_uri must be Google's token endpoint",
        ))
    }

    pub(crate) fn http(&self) -> Result<reqwest::Client> {
        client(self.config.timeout_secs, GOOGLE_CHAT_APP_CONNECTOR_TYPE)
    }

    pub(crate) fn chat_base(&self) -> &str {
        self.config.api_base_url.trim().trim_end_matches('/')
    }

    pub(crate) fn pubsub_base(&self) -> &str {
        self.config.pubsub_base_url.trim().trim_end_matches('/')
    }

    /// Mole Agent 收消息的订阅；没填时不在 Google Chat 里回答。
    pub(crate) fn subscription(&self) -> Option<&str> {
        Some(self.config.subscription.trim()).filter(|subscription| !subscription.is_empty())
    }

    fn token_key(&self, scope: &str) -> String {
        TokenCache::key(
            &format!("{GOOGLE_CHAT_APP_CONNECTOR_TYPE}:{scope}"),
            &self.account.client_email,
            &self.account.private_key,
        )
    }

    /// 范围是 `scope` 的访问令牌，缓存到过期前一分钟。
    pub(crate) async fn token(&self, http: &reqwest::Client, scope: &str) -> Result<String> {
        TOKENS
            .get(&self.token_key(scope), async {
                let assertion = assertion(&self.account, scope, unix_seconds()?)?;

                let response = http_trace::send(
                    http,
                    http.post(self.account.token_uri.trim()).form(&[
                        ("grant_type", JWT_BEARER),
                        ("assertion", assertion.as_str()),
                    ]),
                    HttpTarget::ThirdParty,
                )
                .await
                .map_err(|_| Error::unavailable("google_chat_app token request failed"))?;

                let token: TokenResponse = response.json().await.unwrap_or_default();

                if token.access_token.is_empty() {
                    return Err(Error::unavailable(format!(
                        "google_chat_app token request was rejected: {}",
                        provider_message(&token.error_description)
                    )));
                }

                Ok((token.access_token, token.expires_in.max(60)))
            })
            .await
    }

    /// 令牌被拒绝时丢掉它，下次重新取。
    pub(crate) fn forget_token(&self, scope: &str) {
        TOKENS.forget(&self.token_key(scope));
    }

    /// 一个人与应用的私聊空间：他要先给应用发过消息或装了应用。
    pub(crate) async fn direct_message(
        &self,
        http: &reqwest::Client,
        token: &str,
        user: &str,
    ) -> Result<String> {
        let url = with_query(
            &format!("{}/spaces:findDirectMessage", self.chat_base()),
            &[("name", user_name(user).as_str())],
        )?;

        let found = check(
            http_trace::send(
                http,
                http.get(url).bearer_auth(token),
                HttpTarget::ThirdParty,
            )
            .await
            .map_err(|_| Error::unavailable("google_chat_app request failed"))?,
            "finding the direct message",
        )
        .await?;

        found["name"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| Error::unavailable("google_chat_app found no direct message space"))
    }
}

/// `spaces/AAAA` 或 `AAAA` 都当作空间。
pub(crate) fn space_name(value: &str) -> String {
    let value = value.trim();

    if value.starts_with("spaces/") {
        value.to_string()
    } else {
        format!("spaces/{value}")
    }
}

/// `users/123`、`123` 都当作用户。
fn user_name(value: &str) -> String {
    let value = value.trim();

    if value.starts_with("users/") {
        value.to_string()
    } else {
        format!("users/{value}")
    }
}

/// 接口的回答：不成功时带上 Google 说的原因。
pub(crate) async fn check(response: reqwest::Response, action: &str) -> Result<Value> {
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);

    if !status.is_success() {
        return Err(Error::unavailable(format!(
            "google_chat_app {action} was refused: HTTP {} {}",
            status.as_u16(),
            provider_message(body["error"]["message"].as_str().unwrap_or_default())
        )));
    }

    Ok(body)
}

/// 服务账号的私钥（PEM，PKCS#8 或 PKCS#1）。
fn signing_key(pem: &str) -> Result<RsaKeyPair> {
    let invalid = || Error::invalid("google_chat_app private_key is not an RSA private key");
    let key = rustls_pemfile::private_key(&mut Cursor::new(pem.as_bytes()))
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;

    match key {
        PrivateKeyDer::Pkcs8(der) => RsaKeyPair::from_pkcs8(der.secret_pkcs8_der()),
        PrivateKeyDer::Pkcs1(der) => RsaKeyPair::from_der(der.secret_pkcs1_der()),
        _ => return Err(invalid()),
    }
    .map_err(|_| invalid())
}

fn unix_seconds() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .map_err(|_| Error::internal("system time is before unix epoch"))
}

/// 换令牌用的断言：服务账号签的 JWT（RS256），一小时有效。
fn assertion(account: &ServiceAccount, scope: &str, now: u64) -> Result<String> {
    let header = URL_SAFE_NO_PAD.encode(json!({ "alg": "RS256", "typ": "JWT" }).to_string());
    let claims = URL_SAFE_NO_PAD.encode(
        json!({
            "iss": account.client_email.trim(),
            "scope": scope,
            "aud": account.token_uri.trim(),
            "iat": now,
            "exp": now + 3_600,
        })
        .to_string(),
    );
    let signed = format!("{header}.{claims}");

    let key = signing_key(&account.private_key)?;
    let mut signature = vec![0; key.public_modulus_len()];
    key.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        signed.as_bytes(),
        &mut signature,
    )
    .map_err(|_| Error::internal("google_chat_app could not sign its token request"))?;

    Ok(format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature)))
}

/// 测试时现生成的 2048 位 RSA 私钥（PKCS#8 PEM）。
#[cfg(test)]
pub(crate) fn test_key() -> String {
    use aws_lc_rs::{
        encoding::{AsDer, Pkcs8V1Der},
        rsa::KeySize,
    };
    use base64::engine::general_purpose::STANDARD;

    let key = RsaKeyPair::generate(KeySize::Rsa2048).unwrap();
    let der: Pkcs8V1Der<'static> = key.as_der().unwrap();
    let body = STANDARD.encode(der.as_ref());
    let lines = body
        .as_bytes()
        .chunks(64)
        .map(|line| std::str::from_utf8(line).unwrap())
        .collect::<Vec<_>>()
        .join("\n");

    format!("-----BEGIN PRIVATE KEY-----\n{lines}\n-----END PRIVATE KEY-----\n")
}

#[cfg(test)]
mod tests {
    use aws_lc_rs::signature::{KeyPair, RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey};

    use super::*;

    fn account(key: &str) -> ServiceAccount {
        ServiceAccount {
            client_email: "mole@project.iam.gserviceaccount.com".into(),
            private_key: key.into(),
            token_uri: default_token_uri(),
        }
    }

    #[test]
    fn the_assertion_is_a_jwt_signed_by_the_service_account() {
        let pem = test_key();
        let jwt = assertion(&account(&pem), PUBSUB_SCOPE, 1_700_000_000).unwrap();
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3);

        let claims: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(claims["scope"], PUBSUB_SCOPE);
        assert_eq!(claims["aud"], "https://oauth2.googleapis.com/token");
        assert_eq!(claims["exp"], 1_700_003_600);

        let key = signing_key(&pem).unwrap();
        let public = UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, key.public_key().as_ref());
        let signature = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
        public
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
            .unwrap();
    }

    #[test]
    fn a_subscription_is_a_full_resource_name_and_optional() {
        let credentials = json!({
            "client_email": "mole@project.iam.gserviceaccount.com",
            "private_key": test_key(),
        });

        let app = GoogleChatApp::parse(&json!({ "credentials": credentials })).unwrap();
        assert_eq!(app.subscription(), None);
        assert_eq!(app.pubsub_base(), "https://pubsub.googleapis.com/v1");

        let app = GoogleChatApp::parse(&json!({
            "credentials": credentials,
            "subscription": " projects/ops/subscriptions/mole-chat ",
        }))
        .unwrap();
        assert_eq!(
            app.subscription(),
            Some("projects/ops/subscriptions/mole-chat")
        );

        assert!(
            GoogleChatApp::parse(
                &json!({ "credentials": credentials, "subscription": "mole-chat" })
            )
            .is_err()
        );
    }

    #[test]
    fn spaces_and_users_take_their_resource_names() {
        assert_eq!(space_name("AAAA"), "spaces/AAAA");
        assert_eq!(space_name("spaces/AAAA"), "spaces/AAAA");
        assert_eq!(user_name("123"), "users/123");
        assert_eq!(user_name("users/123"), "users/123");
    }
}
