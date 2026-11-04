// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Microsoft Teams 的 Bot（Bot Framework）：应用 ID 与客户端密码换令牌，往 Bot Connector 的服务
//! 地址发、改消息。
//!
//! 发通知（[`super::TeamsAppConnectorAdapter`]）用连接器里配的服务地址；Mole Agent 在 Teams 里
//! 回答（`infra::im::teams`）用消息带来的 `serviceUrl`，令牌只发给 Bot Framework 的地址
//! （[`TeamsApp::trusts`]）。

use reqwest::Method;
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use super::{
    super::common::{
        TOKENS, TokenCache, client, default_timeout_secs, provider_message, require,
        validate_http_url, validate_timeout,
    },
    TEAMS_APP_CONNECTOR_TYPE,
};
use crate::shared::{
    Error, Result,
    http_trace::{self, HttpTarget},
};

const BOT_FRAMEWORK_SCOPE: &str = "https://api.botframework.com/.default";
/// 美国政府云（GCC High、DoD）的 Bot Framework：令牌向这里要，按这个范围要。
const GOVERNMENT_SCOPE: &str = "https://api.botframework.us/.default";
const GOVERNMENT_LOGIN_BASE_URL: &str = "https://login.microsoftonline.us";
/// 美国政府云（GCC High、DoD）的 Teams 服务地址在这个域名下。
const GOVERNMENT_DOMAIN: &str = ".teams.microsoft.us";
/// Teams 的 Bot Connector 在这些主机上。
const SERVICE_HOSTS: [&str; 1] = ["smba.trafficmanager.net"];
const SERVICE_DOMAINS: [&str; 3] = [
    ".botframework.com",
    ".teams.microsoft.com",
    ".teams.microsoft.us",
];

/// Teams 的云环境与发通知的 Bot Framework 服务地址：全球、GCC，与美国政府云 GCC High、DoD。
/// 第一个是默认的。
pub(crate) const TEAMS_CLOUDS: [(&str, &str); 4] = [
    ("global", "https://smba.trafficmanager.net/teams/"),
    ("gcc", "https://smba.infra.gcc.teams.microsoft.com/teams"),
    (
        "gcc_high",
        "https://smba.infra.gov.teams.microsoft.us/teams",
    ),
    ("dod", "https://smba.infra.dod.teams.microsoft.us/teams"),
];

fn default_service_url() -> String {
    TEAMS_CLOUDS[0].1.into()
}

fn default_login_base_url() -> String {
    "https://login.microsoftonline.com".into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    app_id: String,
    client_secret: String,
    /// Bot 所在的租户；多租户的旧 Bot 填 `botframework.com`。
    tenant_id: String,
    /// Teams 的云环境由它定：全球、GCC，或美国政府云 GCC High、DoD。
    #[serde(default = "default_service_url")]
    service_url: String,
    /// 测试时把换令牌的地址换成本机的服务。正式的配置里没有这一项：地址随云环境定。
    #[cfg(test)]
    #[serde(default)]
    login_base_url: Option<String>,
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

/// Bot Connector 建出来的东西：消息或会话的 ID。
#[derive(Debug, Default, Deserialize)]
pub(crate) struct Resource {
    #[serde(default)]
    pub(crate) id: Option<String>,
}

/// 一个 Teams Bot 的凭据。
pub(crate) struct TeamsApp {
    config: Config,
}

impl TeamsApp {
    pub(crate) fn parse(value: &Value) -> Result<Self> {
        let config: Config = serde_json::from_value(value.clone())
            .map_err(|error| Error::invalid(format!("invalid teams_app config: {error}")))?;

        require(&config.app_id, "teams_app appId")?;
        require(&config.client_secret, "teams_app clientSecret")?;
        require(&config.tenant_id, "teams_app tenantId")?;
        validate_http_url(&config.service_url, "teams_app serviceUrl")?;
        validate_timeout(config.timeout_secs, TEAMS_APP_CONNECTOR_TYPE)?;
        Ok(Self { config })
    }

    pub(crate) fn app_id(&self) -> &str {
        self.config.app_id.trim()
    }

    pub(crate) fn tenant_id(&self) -> &str {
        self.config.tenant_id.trim()
    }

    /// 只收这个租户的消息：租户填的是租户 ID（GUID）时。多租户的旧 Bot（`botframework.com`）与
    /// 填了域名的不限。
    pub(crate) fn single_tenant(&self) -> Option<&str> {
        Some(self.tenant_id()).filter(|tenant| uuid::Uuid::parse_str(tenant).is_ok())
    }

    pub(crate) fn http(&self) -> Result<reqwest::Client> {
        client(self.config.timeout_secs, TEAMS_APP_CONNECTOR_TYPE)
    }

    /// 连接器里配的服务地址：发通知用。
    pub(crate) fn service(&self) -> &str {
        self.config.service_url.trim().trim_end_matches('/')
    }

    /// 服务地址在美国政府云（GCC High、DoD）：令牌向政府云要，收到的请求也按政府云的 Bot Framework 验。
    pub(crate) fn government(&self) -> bool {
        Url::parse(self.service())
            .ok()
            .and_then(|url| url.host_str().map(|host| host.ends_with(GOVERNMENT_DOMAIN)))
            .unwrap_or(false)
    }

    /// 换令牌的地址与范围：全球与 GCC 用公有云的，GCC High 与 DoD 用政府云的。
    fn login(&self) -> (String, &'static str) {
        let (base, scope) = if self.government() {
            (GOVERNMENT_LOGIN_BASE_URL.to_string(), GOVERNMENT_SCOPE)
        } else {
            (default_login_base_url(), BOT_FRAMEWORK_SCOPE)
        };

        (self.test_login().unwrap_or(base), scope)
    }

    /// 测试里换成的换令牌地址。
    #[cfg(test)]
    fn test_login(&self) -> Option<String> {
        self.config
            .login_base_url
            .as_deref()
            .map(str::trim)
            .filter(|base| !base.is_empty())
            .map(str::to_string)
    }

    #[cfg(not(test))]
    fn test_login(&self) -> Option<String> {
        None
    }

    fn token_key(&self) -> String {
        TokenCache::key(
            TEAMS_APP_CONNECTOR_TYPE,
            &self.config.app_id,
            &self.config.client_secret,
        )
    }

    pub(crate) async fn token(&self, http: &reqwest::Client) -> Result<String> {
        TOKENS
            .get(&self.token_key(), async {
                let (base, scope) = self.login();

                let url = format!(
                    "{}/{}/oauth2/v2.0/token",
                    base.trim_end_matches('/'),
                    self.tenant_id()
                );

                let response = http_trace::send(
                    http,
                    http.post(url).form(&[
                        ("grant_type", "client_credentials"),
                        ("client_id", self.app_id()),
                        ("client_secret", self.config.client_secret.trim()),
                        ("scope", scope),
                    ]),
                    HttpTarget::ThirdParty,
                )
                .await
                .map_err(|_| Error::unavailable("teams_app token request failed"))?;

                let token: TokenResponse = response.json().await.unwrap_or_default();

                if token.access_token.is_empty() {
                    return Err(Error::unavailable(format!(
                        "teams_app token request was rejected: {}",
                        provider_message(&token.error_description)
                    )));
                }

                Ok((token.access_token, token.expires_in.max(60)))
            })
            .await
    }

    /// 令牌能不能发给这个服务地址：连接器里配的，或 Bot Framework 的 https 地址。
    pub(crate) fn trusts(&self, service_url: &str) -> bool {
        let Ok(url) = Url::parse(service_url.trim()) else {
            return false;
        };

        if Url::parse(self.service()).is_ok_and(|configured| configured.origin() == url.origin()) {
            return true;
        }

        url.scheme() == "https"
            && url.host_str().is_some_and(|host| {
                SERVICE_HOSTS.contains(&host)
                    || SERVICE_DOMAINS.iter().any(|domain| host.ends_with(domain))
            })
    }

    /// 往 `service` 发一个请求（发消息是 POST，改消息是 PUT）。
    pub(crate) async fn call(
        &self,
        http: &reqwest::Client,
        token: &str,
        method: Method,
        service: &str,
        path: &str,
        body: &Value,
    ) -> Result<Resource> {
        if !self.trusts(service) {
            return Err(Error::invalid(
                "teams_app refuses to send to a service URL outside Bot Framework",
            ));
        }

        let response = http_trace::send(
            http,
            http.request(
                method,
                format!("{}{path}", service.trim().trim_end_matches('/')),
            )
            .bearer_auth(token)
            .json(body),
            HttpTarget::ThirdParty,
        )
        .await
        .map_err(|_| Error::unavailable("teams_app request failed"))?;

        let status = response.status();

        if status == reqwest::StatusCode::UNAUTHORIZED {
            TOKENS.forget(&self.token_key());
        }

        if !status.is_success() {
            let detail: Value = response.json().await.unwrap_or(Value::Null);

            return Err(Error::unavailable(format!(
                "teams_app was refused: HTTP {} {}",
                status.as_u16(),
                provider_message(detail["error"]["message"].as_str().unwrap_or_default())
            )));
        }

        Ok(response.json().await.unwrap_or_default())
    }

    /// 和一个人的一对一会话：没有就建，有就返回同一个。Bot 要已经装给了这个人。
    pub(crate) async fn personal_conversation(
        &self,
        http: &reqwest::Client,
        token: &str,
        service: &str,
        user: &str,
    ) -> Result<String> {
        let tenant = self.tenant_id();

        self.call(
            http,
            token,
            Method::POST,
            service,
            "/v3/conversations",
            &json!({
                "bot": { "id": format!("28:{}", self.app_id()) },
                "members": [{ "id": user }],
                "channelData": { "tenant": { "id": tenant } },
                "tenantId": tenant,
                "isGroup": false,
            }),
        )
        .await?
        .id
        .ok_or_else(|| Error::unavailable("teams_app did not return a conversation"))
    }
}

/// 会话 ID 里有 `:` `@` `;` 等字符，放进路径前要编码。
pub(crate) fn encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(service_url: &str) -> TeamsApp {
        TeamsApp::parse(&json!({
            "appId": "00000000-0000-0000-0000-000000000001",
            "clientSecret": "s",
            "tenantId": "contoso.onmicrosoft.com",
            "serviceUrl": service_url,
        }))
        .unwrap()
    }

    #[test]
    fn tokens_go_only_to_bot_framework() {
        let app = app("https://smba.trafficmanager.net/teams/");

        assert!(app.trusts("https://smba.trafficmanager.net/emea/"));
        assert!(app.trusts("https://smba.infra.gcc.teams.microsoft.com/teams"));
        assert!(!app.trusts("http://smba.trafficmanager.net/teams/"));
        assert!(!app.trusts("https://smba.trafficmanager.net.evil.example/"));
        assert!(!app.trusts("https://evil.example/teams/"));

        let local = self::app("http://127.0.0.1:4100");
        assert!(local.trusts("http://127.0.0.1:4100/"));
        assert!(!local.trusts("http://127.0.0.1:4101/"));
    }

    #[test]
    fn only_a_tenant_id_keeps_other_tenants_out() {
        let tenant = |tenant_id: &str| {
            TeamsApp::parse(&json!({
                "appId": "a",
                "clientSecret": "s",
                "tenantId": tenant_id,
            }))
            .unwrap()
        };

        assert_eq!(
            tenant("72f988bf-86f1-41af-91ab-2d7cd011db47").single_tenant(),
            Some("72f988bf-86f1-41af-91ab-2d7cd011db47")
        );
        assert_eq!(tenant("contoso.onmicrosoft.com").single_tenant(), None);
        assert_eq!(tenant("botframework.com").single_tenant(), None);
    }

    #[test]
    fn a_conversation_id_is_encoded_for_the_path() {
        assert_eq!(
            encode("19:abc@thread.tacv2;messageid=1"),
            "19%3Aabc%40thread.tacv2%3Bmessageid%3D1"
        );
    }

    #[test]
    fn a_government_cloud_signs_in_at_the_government_cloud() {
        for (service_url, government) in [
            ("https://smba.trafficmanager.net/teams/", false),
            ("https://smba.infra.gcc.teams.microsoft.com/teams", false),
            ("https://smba.infra.gov.teams.microsoft.us/teams", true),
            ("https://smba.infra.dod.teams.microsoft.us/teams", true),
        ] {
            let app = app(service_url);
            let (base, scope) = app.login();

            assert_eq!(app.government(), government, "{service_url}");

            if government {
                assert_eq!(base, "https://login.microsoftonline.us");
                assert_eq!(scope, "https://api.botframework.us/.default");
            } else {
                assert_eq!(base, "https://login.microsoftonline.com");
                assert_eq!(scope, "https://api.botframework.com/.default");
            }

            assert!(app.trusts(service_url), "{service_url}");
        }
    }
}
