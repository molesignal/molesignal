// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Jira：Cloud 用 REST v3（描述与评论是 ADF）、账号邮箱 + API Token；Data Center 用 REST v2（纯文本）、
//! 个人访问令牌。只连站点解析出的地址，不跟随跳转。

use std::time::Duration;

use aws_lc_rs::hmac;
use reqwest::{Method, RequestBuilder, header::USER_AGENT};
use serde_json::{Value, json};

use super::{CreatedTicket, TicketDraft, TicketState};
use crate::{
    domain::alerting::ticket::{JiraDeployment, JiraSettings, TicketConnector},
    infra::connectors::dispatcher::http::pinned_client,
    shared::{Error, Result},
};

const TIMEOUT: Duration = Duration::from_secs(20);

pub struct JiraClient {
    settings: JiraSettings,
    token: String,
    client: reqwest::Client,
}

impl JiraClient {
    pub async fn new(connector: &TicketConnector) -> Result<Self> {
        let settings = connector.jira()?;
        let token = connector
            .secret("apiToken")
            .ok_or_else(|| Error::invalid("the Jira connector has no API token"))?
            .to_string();
        let site = url::Url::parse(&settings.site_url)
            .map_err(|error| Error::invalid(format!("Jira site address: {error}")))?;

        Ok(Self {
            client: pinned_client(&site, TIMEOUT).await?,
            settings,
            token,
        })
    }

    fn version(&self) -> &'static str {
        match self.settings.deployment {
            JiraDeployment::Cloud => "3",
            JiraDeployment::DataCenter => "2",
        }
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let url = format!(
            "{}/rest/api/{}{path}",
            self.settings.site_url,
            self.version()
        );
        let builder = self
            .client
            .request(method, url)
            .header(USER_AGENT, "MoleSignal")
            .header("Accept", "application/json");

        match self.settings.deployment {
            JiraDeployment::Cloud => builder.basic_auth(&self.settings.email, Some(&self.token)),
            JiraDeployment::DataCenter => builder.bearer_auth(&self.token),
        }
    }

    async fn send(&self, builder: RequestBuilder) -> Result<Value> {
        let response = builder
            .send()
            .await
            .map_err(|error| Error::unavailable(format!("Jira could not be reached: {error}")))?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(Error::unavailable(format!(
                "Jira answered {status}: {}",
                jira_error(&text)
            )));
        }

        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    /// 凭证能用：返回登录的人的名字。
    pub async fn check(&self) -> Result<String> {
        let me = self.send(self.request(Method::GET, "/myself")).await?;

        Ok(me["displayName"].as_str().unwrap_or_default().to_string())
    }

    pub async fn create(&self, draft: &TicketDraft) -> Result<CreatedTicket> {
        let body = self.issue_body(draft);
        let created = self
            .send(self.request(Method::POST, "/issue").json(&body))
            .await?;

        let key = created["key"]
            .as_str()
            .ok_or_else(|| Error::unavailable("Jira did not return the issue key"))?
            .to_string();

        Ok(CreatedTicket {
            external_id: created["id"].as_str().unwrap_or(&key).to_string(),
            url: format!("{}/browse/{key}", self.settings.site_url),
            external_key: key,
        })
    }

    pub async fn state(&self, key: &str) -> Result<TicketState> {
        let issue = self
            .send(self.request(
                Method::GET,
                &format!("/issue/{}?fields=status", encode(key)),
            ))
            .await?;

        Ok(state_of(&issue))
    }

    pub async fn comment(&self, key: &str, text: &str) -> Result<()> {
        let body = match self.settings.deployment {
            JiraDeployment::Cloud => json!({ "body": adf(&[text.to_string()], None) }),
            JiraDeployment::DataCenter => json!({ "body": text }),
        };

        self.send(
            self.request(Method::POST, &format!("/issue/{}/comment", encode(key)))
                .json(&body),
        )
        .await?;

        Ok(())
    }

    /// 流转到完结：设了流转就走它（按 ID 或名字），没设就走第一个通往完结类状态的。返回走的流转名。
    pub async fn finish(&self, key: &str) -> Result<Option<String>> {
        let listed = self
            .send(self.request(Method::GET, &format!("/issue/{}/transitions", encode(key))))
            .await?;

        let Some((id, name)) = pick_transition(&listed, &self.settings.resolve_transition) else {
            return Ok(None);
        };

        self.send(
            self.request(Method::POST, &format!("/issue/{}/transitions", encode(key)))
                .json(&json!({ "transition": { "id": id } })),
        )
        .await?;

        Ok(Some(name))
    }

    fn issue_body(&self, draft: &TicketDraft) -> Value {
        let mut fields = self.settings.fields.clone();

        fields.insert(
            "project".into(),
            json!({ "key": self.settings.project_key }),
        );
        fields.insert(
            "issuetype".into(),
            json!({ "name": self.settings.issue_type }),
        );
        fields.insert(
            "summary".into(),
            json!(draft.summary.chars().take(250).collect::<String>()),
        );
        fields.insert(
            "description".into(),
            match self.settings.deployment {
                JiraDeployment::Cloud => adf(&draft.lines, Some(&draft.link)),
                JiraDeployment::DataCenter => {
                    json!(format!("{}\n{}", draft.lines.join("\n"), draft.link))
                }
            },
        );

        let labels = self
            .settings
            .labels
            .iter()
            .chain(draft.labels.iter())
            .map(|label| label.replace(char::is_whitespace, "_"))
            .collect::<Vec<_>>();

        if !labels.is_empty() {
            fields.insert("labels".into(), json!(labels));
        }

        if let Some(priority) = draft.mapped_priority(&self.settings.priorities) {
            fields.insert("priority".into(), json!({ "name": priority }));
        }

        json!({ "fields": fields })
    }
}

/// Jira 的 Webhook 签名：`X-Hub-Signature: sha256=<hex>`，HMAC-SHA256 对原始请求体。
pub fn signature_matches(secret: &str, body: &[u8], presented: &str) -> bool {
    let Some(hex_digest) = presented.trim().strip_prefix("sha256=") else {
        return false;
    };

    let Ok(expected) = hex::decode(hex_digest) else {
        return false;
    };

    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    hmac::verify(&key, body, &expected).is_ok()
}

/// Webhook 里的 issue：key 与现在的状态。
pub fn webhook_issue(body: &Value) -> Option<(String, TicketState)> {
    let issue = body.get("issue")?;
    let key = issue["key"].as_str()?.to_string();

    Some((key, state_of(issue)))
}

fn state_of(issue: &Value) -> TicketState {
    let status = &issue["fields"]["status"];

    TicketState {
        status: status["name"].as_str().unwrap_or_default().to_string(),
        done: status["statusCategory"]["key"].as_str() == Some("done"),
    }
}

fn pick_transition(listed: &Value, wanted: &str) -> Option<(String, String)> {
    let transitions = listed["transitions"].as_array()?;
    let wanted = wanted.trim();

    let chosen = if wanted.is_empty() {
        transitions
            .iter()
            .find(|transition| transition["to"]["statusCategory"]["key"].as_str() == Some("done"))
    } else {
        transitions.iter().find(|transition| {
            transition["id"].as_str() == Some(wanted)
                || transition["name"]
                    .as_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case(wanted))
        })
    }?;

    Some((
        chosen["id"].as_str()?.to_string(),
        chosen["name"].as_str().unwrap_or_default().to_string(),
    ))
}

/// 段落写成 Atlassian 文档格式；`link` 是最后一段的链接。
fn adf(lines: &[String], link: Option<&str>) -> Value {
    let mut content = lines
        .iter()
        .filter(|line| !line.is_empty())
        .map(|line| json!({ "type": "paragraph", "content": [{ "type": "text", "text": line }] }))
        .collect::<Vec<_>>();

    if let Some(link) = link.filter(|link| !link.is_empty()) {
        content.push(json!({
            "type": "paragraph",
            "content": [{ "type": "text", "text": link, "marks": [{ "type": "link", "attrs": { "href": link } }] }]
        }));
    }

    json!({ "type": "doc", "version": 1, "content": content })
}

fn jira_error(text: &str) -> String {
    let parsed: Value = serde_json::from_str(text).unwrap_or(Value::Null);

    let mut messages = parsed["errorMessages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();

    if let Some(errors) = parsed["errors"].as_object() {
        messages.extend(
            errors.iter().map(|(field, message)| {
                format!("{field}: {}", message.as_str().unwrap_or_default())
            }),
        );
    }

    if messages.is_empty() {
        text.chars().take(300).collect()
    } else {
        messages.join("; ")
    }
}

fn encode(key: &str) -> String {
    key.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_webhook_signature_is_the_hmac_of_the_raw_body() {
        let body = br#"{"issue":{"key":"OPS-1"}}"#;
        let key = hmac::Key::new(hmac::HMAC_SHA256, b"shh");
        let signed = format!("sha256={}", hex::encode(hmac::sign(&key, body).as_ref()));

        assert!(signature_matches("shh", body, &signed));
        assert!(!signature_matches("other", body, &signed));
        assert!(!signature_matches("shh", body, "md5=abc"));
    }

    #[test]
    fn the_issue_and_its_status_category_come_from_the_webhook() {
        let body = json!({
            "webhookEvent": "jira:issue_updated",
            "issue": { "key": "OPS-42", "fields": { "status": { "name": "Done", "statusCategory": { "key": "done" } } } }
        });

        let (key, state) = webhook_issue(&body).unwrap();
        assert_eq!(key, "OPS-42");
        assert!(state.done);
        assert_eq!(state.status, "Done");
    }

    #[test]
    fn the_resolve_transition_is_the_named_one_or_the_first_to_done() {
        let listed = json!({ "transitions": [
            { "id": "11", "name": "Start", "to": { "statusCategory": { "key": "indeterminate" } } },
            { "id": "31", "name": "Resolve", "to": { "statusCategory": { "key": "done" } } },
            { "id": "41", "name": "Won't do", "to": { "statusCategory": { "key": "done" } } }
        ]});

        assert_eq!(
            pick_transition(&listed, ""),
            Some(("31".into(), "Resolve".into()))
        );
        assert_eq!(
            pick_transition(&listed, "won't DO"),
            Some(("41".into(), "Won't do".into()))
        );
        assert_eq!(
            pick_transition(&listed, "11"),
            Some(("11".into(), "Start".into()))
        );
        assert_eq!(pick_transition(&listed, "missing"), None);
    }

    #[test]
    fn jira_errors_are_read_from_its_answer() {
        assert_eq!(
            jira_error(r#"{"errorMessages":["No project"],"errors":{"summary":"too long"}}"#),
            "No project; summary: too long"
        );
    }
}
