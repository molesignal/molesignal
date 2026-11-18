// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Google Chat 应用：服务账号的密钥换访问令牌，Chat 与 Pub/Sub 的接口地址。
//!
//! 发通知（[`super::GoogleChatAppConnectorAdapter`]）与 Mole Agent 在 Google Chat 里回答
//! （`infra::im::google_chat`）都用它。令牌按范围分开缓存：发消息用 `chat.bot`，从 Pub/Sub
//! 订阅里拉消息用 `pubsub`。

use kernel::{Error, Result};
use serde::Deserialize;
use serde_json::Value;
use signals::http_trace::{self, HttpTarget};

use super::{
    super::{
        common::{
            TOKENS, TokenCache, client, default_timeout_secs, provider_message, validate_http_url,
            validate_timeout, with_query,
        },
        google_account::ServiceAccount,
    },
    GOOGLE_CHAT_APP_CONNECTOR_TYPE,
};

pub(crate) const CHAT_BOT_SCOPE: &str = "https://www.googleapis.com/auth/chat.bot";
pub(crate) const PUBSUB_SCOPE: &str = "https://www.googleapis.com/auth/pubsub";

pub(crate) const GOOGLE_CHAT_API_BASE_URL: &str = "https://chat.googleapis.com/v1";
/// Mole Agent 收消息的 Pub/Sub。
pub(crate) const GOOGLE_PUBSUB_BASE_URL: &str = "https://pubsub.googleapis.com/v1";

fn default_api_base_url() -> String {
    GOOGLE_CHAT_API_BASE_URL.into()
}

fn default_pubsub_base_url() -> String {
    GOOGLE_PUBSUB_BASE_URL.into()
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

/// 一个 Chat 应用的服务账号与接口地址。
pub(crate) struct GoogleChatApp {
    config: Config,
    account: ServiceAccount,
}

impl GoogleChatApp {
    pub(crate) fn parse(value: &Value) -> Result<Self> {
        let config: Config = serde_json::from_value(value.clone())
            .map_err(|error| Error::invalid(format!("invalid google_chat_app config: {error}")))?;

        let account = ServiceAccount::parse(&config.credentials, GOOGLE_CHAT_APP_CONNECTOR_TYPE)?;

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
        self.account
            .validate_token_uri(GOOGLE_CHAT_APP_CONNECTOR_TYPE)
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
            .get(
                &self.token_key(scope),
                self.account
                    .fetch_token(http, scope, None, GOOGLE_CHAT_APP_CONNECTOR_TYPE),
            )
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::infra::notify::adapters::google_account::test_key;

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
