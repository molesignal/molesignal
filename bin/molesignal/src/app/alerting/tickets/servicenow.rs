// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ServiceNow：Table API（`/api/now/table/{table}`）。默认表是 `incident`，也可以是继承 `task` 的别的表。
//!
//! 认证三选一：基本认证、OAuth 客户端凭证（`/oauth_token.do`，实例要打开客户端凭证授权）、
//! API Key（`x-sn-apikey`，Washington DC 起）。只连实例的地址，不跟随跳转。
//!
//! - 优先级不能直接写：ServiceNow 按 impact 和 urgency 算，映射里写 `impact/urgency`（如 `2/1`），只写一个
//!   数字时两个都用它。
//! - 状态按 `state` 的值判断完结（默认 6 已解决、7 已关闭、8 已取消）。
//! - 解决时写 `state`、`close_code`、`close_notes`：默认的数据策略要求后两个。
//! - 事件上的变化写进 `work_notes`（内部备注），不打扰提单人。

use std::time::Duration;

use reqwest::{Method, RequestBuilder, header::USER_AGENT};
use serde_json::{Map, Value, json};

use super::{CreatedTicket, TicketDraft, TicketState, meegle::PluginToken};
use crate::{
    domain::alerting::ticket::{
        ServiceNowAuth, ServiceNowSettings, TicketConnector, servicenow_impact_and_urgency,
    },
    infra::connectors::dispatcher::http::pinned_client,
    shared::{Error, Result},
};

const TIMEOUT: Duration = Duration::from_secs(20);

/// `short_description` 默认的字段长度。
const MAX_SHORT_DESCRIPTION_CHARS: usize = 160;

/// 这个连接器怎么证明身份。
enum Credential {
    Basic { username: String, password: String },
    Bearer(String),
    ApiKey(String),
}

pub struct ServiceNowClient {
    settings: ServiceNowSettings,
    credential: Credential,
    client: reqwest::Client,
}

impl ServiceNowClient {
    /// 用缓存的 OAuth 令牌，过期了就换一个；返回客户端和（换了时）新的令牌。别的认证方式没有令牌。
    pub async fn connect(
        connector: &TicketConnector,
        cached: Option<PluginToken>,
        now_secs: i64,
    ) -> Result<(Self, Option<PluginToken>)> {
        let settings = connector.servicenow()?;
        let instance = url::Url::parse(&settings.instance_url)
            .map_err(|error| Error::invalid(format!("ServiceNow instance address: {error}")))?;
        let client = pinned_client(&instance, TIMEOUT).await?;

        let (credential, renewed) = match settings.auth {
            ServiceNowAuth::Basic => {
                let password = connector
                    .secret("password")
                    .ok_or_else(|| Error::invalid("the ServiceNow connector has no password"))?;

                (
                    Credential::Basic {
                        username: settings.username.clone(),
                        password: password.to_string(),
                    },
                    None,
                )
            }
            ServiceNowAuth::ApiKey => {
                let key = connector
                    .secret("apiKey")
                    .ok_or_else(|| Error::invalid("the ServiceNow connector has no API key"))?;

                (Credential::ApiKey(key.to_string()), None)
            }
            ServiceNowAuth::Oauth => match cached.filter(|token| token.fresh(now_secs)) {
                Some(token) => (Credential::Bearer(token.token), None),
                None => {
                    let secret = connector.secret("clientSecret").ok_or_else(|| {
                        Error::invalid("the ServiceNow connector has no client secret")
                    })?;

                    let token = fetch_token(&client, &settings, secret, now_secs).await?;

                    (Credential::Bearer(token.token.clone()), Some(token))
                }
            },
        };

        Ok((
            Self {
                settings,
                credential,
                client,
            },
            renewed,
        ))
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let url = format!(
            "{}/api/now/table/{}{path}",
            self.settings.instance_url, self.settings.table
        );

        let builder = self
            .client
            .request(method, url)
            .header(USER_AGENT, "MoleSignal")
            .header("Accept", "application/json");

        match &self.credential {
            Credential::Basic { username, password } => {
                builder.basic_auth(username, Some(password))
            }
            Credential::Bearer(token) => builder.bearer_auth(token),
            Credential::ApiKey(key) => builder.header("x-sn-apikey", key),
        }
    }

    async fn send(&self, builder: RequestBuilder) -> Result<Value> {
        let response = builder.send().await.map_err(|error| {
            Error::unavailable(format!("ServiceNow could not be reached: {error}"))
        })?;

        let status = response.status();
        let text = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(Error::unavailable(format!(
                "ServiceNow answered {status}: {}",
                servicenow_error(&text)
            )));
        }

        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    /// 凭证能用，这张表读得了。返回一句说明。
    pub async fn check(&self) -> Result<String> {
        self.send(self.request(Method::GET, "?sysparm_limit=1&sysparm_fields=sys_id"))
            .await?;

        Ok(self.settings.table.clone())
    }

    pub async fn create(&self, draft: &TicketDraft) -> Result<CreatedTicket> {
        let created = self
            .send(
                self.request(
                    Method::POST,
                    "?sysparm_fields=sys_id,number&sysparm_exclude_reference_link=true",
                )
                .json(&self.record_body(draft)),
            )
            .await?;

        let record = &created["result"];

        let sys_id = record["sys_id"]
            .as_str()
            .ok_or_else(|| Error::unavailable("ServiceNow did not return the record's sys_id"))?
            .to_string();

        let number = record["number"].as_str().unwrap_or(&sys_id).to_string();

        Ok(CreatedTicket {
            url: format!(
                "{}/nav_to.do?uri={}.do?sys_id={sys_id}",
                self.settings.instance_url, self.settings.table
            ),
            external_id: sys_id,
            external_key: number,
        })
    }

    pub async fn state(&self, sys_id: &str) -> Result<TicketState> {
        let read = self
            .send(self.request(
                Method::GET,
                &format!(
                    "/{}?sysparm_fields=state&sysparm_display_value=all",
                    encode(sys_id)
                ),
            ))
            .await?;

        Ok(state_of(
            &read["result"]["state"],
            &self.settings.done_states,
        ))
    }

    /// 写进内部备注。
    pub async fn comment(&self, sys_id: &str, text: &str) -> Result<()> {
        self.send(
            self.request(Method::PATCH, &format!("/{}", encode(sys_id)))
                .json(&json!({ "work_notes": text })),
        )
        .await?;

        Ok(())
    }

    /// 解决记录：写状态、解决代码和解决说明。
    pub async fn finish(&self, sys_id: &str, note: &str) -> Result<()> {
        let mut body = Map::new();

        body.insert("state".into(), json!(self.settings.resolve_state));
        body.insert("close_notes".into(), json!(note));

        if !self.settings.close_code.trim().is_empty() {
            body.insert("close_code".into(), json!(self.settings.close_code));
        }

        self.send(
            self.request(Method::PATCH, &format!("/{}", encode(sys_id)))
                .json(&Value::Object(body)),
        )
        .await?;

        Ok(())
    }

    fn record_body(&self, draft: &TicketDraft) -> Value {
        let mut fields = self.settings.fields.clone();

        fields.insert(
            "short_description".into(),
            json!(
                draft
                    .summary
                    .chars()
                    .take(MAX_SHORT_DESCRIPTION_CHARS)
                    .collect::<String>()
            ),
        );
        fields.insert(
            "description".into(),
            json!(format!("{}\n{}", draft.lines.join("\n"), draft.link)),
        );
        fields.insert("correlation_id".into(), json!(draft.incident_id));
        fields.insert("correlation_display".into(), json!("MoleSignal"));

        for (name, value) in [
            ("assignment_group", &self.settings.assignment_group),
            ("caller_id", &self.settings.caller_id),
            ("category", &self.settings.category),
        ] {
            if !value.trim().is_empty() {
                fields.insert(name.into(), json!(value.trim()));
            }
        }

        if let Some((impact, urgency)) = draft
            .mapped_priority(&self.settings.priorities)
            .and_then(|mapped| servicenow_impact_and_urgency(mapped))
        {
            fields.insert("impact".into(), json!(impact));
            fields.insert("urgency".into(), json!(urgency));
        }

        Value::Object(fields)
    }
}

/// 换 OAuth 令牌：客户端凭证授权。
async fn fetch_token(
    client: &reqwest::Client,
    settings: &ServiceNowSettings,
    secret: &str,
    now_secs: i64,
) -> Result<PluginToken> {
    let response = client
        .post(format!("{}/oauth_token.do", settings.instance_url))
        .header(USER_AGENT, "MoleSignal")
        .form(&[
            ("grant_type", "client_credentials"),
            ("client_id", settings.client_id.as_str()),
            ("client_secret", secret),
        ])
        .send()
        .await
        .map_err(|error| Error::unavailable(format!("ServiceNow could not be reached: {error}")))?;

    let status = response.status();
    let answer: Value = response.json().await.unwrap_or_default();

    let Some(token) = answer["access_token"]
        .as_str()
        .filter(|_| status.is_success())
    else {
        let reason = answer["error_description"]
            .as_str()
            .or_else(|| answer["error"].as_str())
            .unwrap_or_default();

        return Err(Error::unavailable(format!(
            "ServiceNow would not give a token ({status}): {reason}"
        )));
    };

    Ok(PluginToken {
        token: token.to_string(),
        expires_at: now_secs + answer["expires_in"].as_i64().unwrap_or(1800),
    })
}

/// `state` 字段（`sysparm_display_value=all`）：`{ "display_value": "已解决", "value": "6" }`。
fn state_of(state: &Value, done_states: &[String]) -> TicketState {
    let value = state["value"].as_str().unwrap_or_default();

    TicketState {
        status: state["display_value"]
            .as_str()
            .filter(|name| !name.is_empty())
            .unwrap_or(value)
            .to_string(),
        done: done_states.iter().any(|done| done.trim() == value),
    }
}

/// `{ "error": { "message": …, "detail": … }, "status": "failure" }`。
fn servicenow_error(text: &str) -> String {
    let parsed: Value = serde_json::from_str(text).unwrap_or(Value::Null);

    let message = parsed["error"]["message"].as_str().unwrap_or_default();
    let detail = parsed["error"]["detail"].as_str().unwrap_or_default();

    match (message.is_empty(), detail.is_empty()) {
        (false, false) => format!("{message}: {detail}"),
        (false, true) => message.to_string(),
        (true, false) => detail.to_string(),
        (true, true) => text.chars().take(300).collect(),
    }
}

fn encode(sys_id: &str) -> String {
    sys_id.chars().filter(char::is_ascii_alphanumeric).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{
            basic_auth, body_json, body_partial_json, body_string_contains, header, method, path,
            query_param,
        },
    };

    use super::*;
    use crate::{
        domain::alerting::ticket::{TicketAutoRule, TicketComments, TicketSystem},
        shared::{ids::Id, time::TimestampMicros},
    };

    fn connector(instance: &str, auth: &str, secrets: &[(&str, &str)]) -> TicketConnector {
        TicketConnector {
            id: Id::from_string("tc-1"),
            org_id: Id::from_string("org-1"),
            name: "Ops ServiceNow".into(),
            system: TicketSystem::Servicenow,
            settings: json!({
                "instanceUrl": instance,
                "auth": auth,
                "username": "molesignal",
                "clientId": "client",
                "assignmentGroup": "287ebd7da9fe198100f92cc8d1d2fbcd",
                "category": "software",
                "priorities": { "critical": "1/1", "warning": "3", "P2": "2/1" },
                "fields": { "contact_type": "monitoring" },
                "closeCode": "Solution provided",
                "doneStates": ["6", "7", "8"],
            }),
            secrets: secrets
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect::<BTreeMap<_, _>>(),
            secrets_set: Vec::new(),
            comments: TicketComments::default(),
            transition_on_resolve: true,
            resolve_incident_when_done: false,
            auto: TicketAutoRule::default(),
            on_major_incident: false,
            enabled: true,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn draft(priority: Option<&str>) -> TicketDraft {
        TicketDraft {
            summary: "[critical] Checkout errors".into(),
            lines: vec!["Checkout errors".into(), "severity: critical".into()],
            link: "https://molesignal.example.com/alerts/incidents/inc-1".into(),
            severity: "critical".into(),
            priority: priority.map(str::to_string),
            labels: vec!["molesignal".into()],
            incident_id: "inc-1".into(),
        }
    }

    async fn basic(server: &MockServer) -> ServiceNowClient {
        ServiceNowClient::connect(
            &connector(&server.uri(), "basic", &[("password", "secret")]),
            None,
            1000,
        )
        .await
        .unwrap()
        .0
    }

    #[tokio::test]
    async fn a_ticket_is_a_record_in_the_table_with_impact_and_urgency() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/now/table/incident"))
            .and(basic_auth("molesignal", "secret"))
            .and(query_param("sysparm_exclude_reference_link", "true"))
            .and(body_partial_json(json!({
                "short_description": "[critical] Checkout errors",
                "correlation_id": "inc-1",
                "correlation_display": "MoleSignal",
                "assignment_group": "287ebd7da9fe198100f92cc8d1d2fbcd",
                "category": "software",
                "contact_type": "monitoring",
                "impact": "1",
                "urgency": "1",
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "result": { "sys_id": "abc123", "number": "INC0010042" }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let created = basic(&server).await.create(&draft(None)).await.unwrap();

        assert_eq!(created.external_id, "abc123");
        assert_eq!(created.external_key, "INC0010042");
        assert_eq!(
            created.url,
            format!("{}/nav_to.do?uri=incident.do?sys_id=abc123", server.uri())
        );
    }

    #[test]
    fn the_priority_of_the_incident_comes_before_its_severity_and_one_number_means_both() {
        assert_eq!(
            servicenow_impact_and_urgency("2/1"),
            Some(("2".to_string(), "1".to_string()))
        );
        assert_eq!(
            servicenow_impact_and_urgency(" 3 "),
            Some(("3".to_string(), "3".to_string()))
        );
        assert_eq!(servicenow_impact_and_urgency(""), None);

        let priorities = BTreeMap::from([
            ("critical".to_string(), "1/1".to_string()),
            ("P2".to_string(), "2/1".to_string()),
        ]);

        assert_eq!(
            draft(Some("P2"))
                .mapped_priority(&priorities)
                .map(String::as_str),
            Some("2/1")
        );
        assert_eq!(
            draft(Some("P5"))
                .mapped_priority(&priorities)
                .map(String::as_str),
            Some("1/1")
        );
    }

    #[tokio::test]
    async fn the_state_is_read_and_compared_by_its_value() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident/abc123"))
            .and(query_param("sysparm_display_value", "all"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": { "state": { "display_value": "已解决", "value": "6" } }
            })))
            .mount(&server)
            .await;

        let state = basic(&server).await.state("abc123").await.unwrap();

        assert_eq!(state.status, "已解决");
        assert!(state.done);

        let done = vec!["6".to_string()];
        let open = state_of(&json!({ "display_value": "", "value": "2" }), &done);
        assert_eq!(open.status, "2");
        assert!(!open.done);
    }

    #[tokio::test]
    async fn changes_go_to_the_work_notes_and_a_resolve_sets_the_state_and_the_close_fields() {
        let server = MockServer::start().await;

        Mock::given(method("PATCH"))
            .and(path("/api/now/table/incident/abc123"))
            .and(body_json(json!({ "work_notes": "Acknowledged." })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "result": {} })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/api/now/table/incident/abc123"))
            .and(body_json(json!({
                "state": "6",
                "close_code": "Solution provided",
                "close_notes": "Resolved.",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "result": {} })))
            .expect(1)
            .mount(&server)
            .await;

        let client = basic(&server).await;

        client.comment("abc123", "Acknowledged.").await.unwrap();
        client.finish("abc123", "Resolved.").await.unwrap();
    }

    #[tokio::test]
    async fn oauth_takes_a_token_with_client_credentials_and_reuses_it() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/oauth_token.do"))
            .and(body_string_contains("grant_type=client_credentials"))
            .and(body_string_contains("client_id=client"))
            .and(body_string_contains("client_secret=shh"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "tok-1", "expires_in": 1800
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(header("authorization", "Bearer tok-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "result": [] })))
            .expect(2)
            .mount(&server)
            .await;

        let connector = connector(&server.uri(), "oauth", &[("clientSecret", "shh")]);

        let (client, renewed) = ServiceNowClient::connect(&connector, None, 1000)
            .await
            .unwrap();
        let renewed = renewed.expect("a new token");
        assert_eq!(renewed.expires_at, 2800);
        assert_eq!(client.check().await.unwrap(), "incident");

        let (client, again) = ServiceNowClient::connect(&connector, Some(renewed), 1100)
            .await
            .unwrap();
        assert!(again.is_none());
        client.check().await.unwrap();
    }

    #[tokio::test]
    async fn an_api_key_goes_in_its_header() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(header("x-sn-apikey", "key-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "result": [] })))
            .expect(1)
            .mount(&server)
            .await;

        let connector = connector(&server.uri(), "api_key", &[("apiKey", "key-1")]);
        let (client, _) = ServiceNowClient::connect(&connector, None, 1000)
            .await
            .unwrap();

        client.check().await.unwrap();
    }

    #[tokio::test]
    async fn a_refusal_says_what_servicenow_said() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "error": { "message": "Operation Failed", "detail": "ACL Exception Insert Failed" },
                "status": "failure",
            })))
            .mount(&server)
            .await;

        let error = basic(&server)
            .await
            .create(&draft(None))
            .await
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("Operation Failed: ACL Exception Insert Failed"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn a_missing_secret_is_said_before_anything_is_sent() {
        let server = MockServer::start().await;

        let missing =
            ServiceNowClient::connect(&connector(&server.uri(), "basic", &[]), None, 1).await;

        assert!(missing.is_err());
    }
}
