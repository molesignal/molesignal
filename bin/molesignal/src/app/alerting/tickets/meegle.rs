// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 飞书项目（Meegle）的开放接口：插件凭证换 Token（`POST /open_api/authen/plugin_token`，提前 3 分钟
//! 换新），请求带 `X-PLUGIN-TOKEN` 与以谁的身份的 `X-USER-KEY`。建工作项、按 ID 查状态、写评论、
//! 状态流的流转或节点流的完成节点。回答是 `{err_code, err_msg, data}`，`err_code` 不为 0 算失败。

use std::time::Duration;

use reqwest::{Method, RequestBuilder, header::USER_AGENT};
use serde_json::{Value, json};

use super::{CreatedTicket, TicketDraft, TicketState};
use crate::{
    domain::alerting::ticket::{MeegleSettings, TicketConnector},
    infra::connectors::dispatcher::http::pinned_client,
    shared::{Error, Result},
};

const TIMEOUT: Duration = Duration::from_secs(20);

/// Token 提前多久换新。
const REFRESH_EARLY_SECS: i64 = 180;

/// 换来的 Token 与它到期的时间（Unix 秒）。
#[derive(Debug, Clone)]
pub struct PluginToken {
    pub token: String,
    pub expires_at: i64,
}

impl PluginToken {
    pub fn fresh(&self, now_secs: i64) -> bool {
        now_secs + REFRESH_EARLY_SECS < self.expires_at
    }
}

pub struct MeegleClient {
    settings: MeegleSettings,
    token: String,
    client: reqwest::Client,
}

impl MeegleClient {
    /// 用缓存的 Token，过期了就换一个；返回客户端和（换了时）新的 Token。
    pub async fn connect(
        connector: &TicketConnector,
        cached: Option<PluginToken>,
        now_secs: i64,
    ) -> Result<(Self, Option<PluginToken>)> {
        let settings = connector.meegle()?;
        let base = url::Url::parse(settings.site.base_url())
            .map_err(|error| Error::invalid(format!("Meegle site address: {error}")))?;
        let client = pinned_client(&base, TIMEOUT).await?;

        let (token, renewed) = match cached.filter(|token| token.fresh(now_secs)) {
            Some(token) => (token.token, None),
            None => {
                let secret = connector
                    .secret("pluginSecret")
                    .ok_or_else(|| Error::invalid("the Meegle connector has no plugin secret"))?;
                let issued = plugin_token(&client, &settings, secret, now_secs).await?;
                (issued.token.clone(), Some(issued))
            }
        };

        Ok((
            Self {
                settings,
                token,
                client,
            },
            renewed,
        ))
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        self.client
            .request(
                method,
                format!("{}/open_api{path}", self.settings.site.base_url()),
            )
            .header(USER_AGENT, "MoleSignal")
            .header("X-PLUGIN-TOKEN", &self.token)
            .header("X-USER-KEY", &self.settings.user_key)
    }

    fn item_path(&self) -> String {
        format!(
            "/{}/work_item/{}",
            self.settings.project_key, self.settings.work_item_type_key
        )
    }

    /// 凭证与空间、类型都对：读得到工作项类型的元数据。
    pub async fn check(&self) -> Result<()> {
        send(self.request(Method::GET, &format!("{}/meta", self.item_path()))).await?;
        Ok(())
    }

    pub async fn create(&self, draft: &TicketDraft) -> Result<CreatedTicket> {
        let body = self.create_body(draft);
        let data = send(
            self.request(
                Method::POST,
                &format!("/{}/work_item/create", self.settings.project_key),
            )
            .json(&body),
        )
        .await?;

        let id = data
            .as_i64()
            .map(|id| id.to_string())
            .or_else(|| data.as_str().map(str::to_string))
            .ok_or_else(|| Error::unavailable("Meegle did not return the work item ID"))?;

        Ok(CreatedTicket {
            url: format!(
                "{}/{}/{}/detail/{id}",
                self.settings.site.base_url(),
                self.settings.project_key,
                self.settings.work_item_type_key
            ),
            external_key: id.clone(),
            external_id: id,
        })
    }

    pub async fn state(&self, id: &str) -> Result<TicketState> {
        let data = send(
            self.request(Method::POST, &format!("{}/query", self.item_path()))
                .json(&json!({ "work_item_ids": [work_item_id(id)?] })),
        )
        .await?;

        Ok(state_of(&data, &self.settings.done_states))
    }

    pub async fn comment(&self, id: &str, text: &str) -> Result<()> {
        send(
            self.request(
                Method::POST,
                &format!("{}/{}/comment/create", self.item_path(), work_item_id(id)?),
            )
            .json(&json!({ "content": text })),
        )
        .await?;

        Ok(())
    }

    /// 解决事件时：状态流走设好的流转，节点流完成设好的节点。都没设时不动，返回 `false`。
    pub async fn finish(&self, id: &str) -> Result<bool> {
        let workflow = format!(
            "/{}/workflow/{}/{}",
            self.settings.project_key,
            self.settings.work_item_type_key,
            work_item_id(id)?
        );

        if let Some(transition) = self.settings.resolve_transition_id {
            send(
                self.request(Method::POST, &format!("{workflow}/node/state_change"))
                    .json(&json!({ "transition_id": transition })),
            )
            .await?;

            return Ok(true);
        }

        let node = self.settings.resolve_node_id.trim();

        if node.is_empty() {
            return Ok(false);
        }

        send(
            self.request(Method::POST, &format!("{workflow}/node/{node}/operate"))
                .json(&json!({ "action": "confirm" })),
        )
        .await?;

        Ok(true)
    }

    fn create_body(&self, draft: &TicketDraft) -> Value {
        let mut pairs = self
            .settings
            .fields
            .iter()
            .map(|(key, value)| json!({ "field_key": key, "field_value": value }))
            .collect::<Vec<_>>();

        pairs.push(json!({
            "field_key": "description",
            "field_value": format!("{}\n{}", draft.lines.join("\n"), draft.link),
        }));

        let priority_field = self.settings.priority_field.trim();

        if !priority_field.is_empty()
            && let Some(value) = draft.mapped_priority(&self.settings.priorities)
        {
            pairs.push(json!({ "field_key": priority_field, "field_value": { "value": value } }));
        }

        json!({
            "work_item_type_key": self.settings.work_item_type_key,
            "name": draft.summary.chars().take(200).collect::<String>(),
            "field_value_pairs": pairs,
        })
    }
}

async fn plugin_token(
    client: &reqwest::Client,
    settings: &MeegleSettings,
    secret: &str,
    now_secs: i64,
) -> Result<PluginToken> {
    let data = send(
        client
            .post(format!(
                "{}/open_api/authen/plugin_token",
                settings.site.base_url()
            ))
            .header(USER_AGENT, "MoleSignal")
            .json(&json!({
                "plugin_id": settings.plugin_id,
                "plugin_secret": secret,
                "type": 0,
            })),
    )
    .await?;

    let token = data["token"]
        .as_str()
        .filter(|token| !token.is_empty())
        .ok_or_else(|| Error::unavailable("Meegle did not issue a plugin token"))?;

    Ok(PluginToken {
        token: token.to_string(),
        expires_at: now_secs + data["expire_time"].as_i64().unwrap_or(7_200),
    })
}

/// 发请求，读出 `data`；`err_code` 不为 0 或状态码不是 2xx 时报错。
async fn send(builder: RequestBuilder) -> Result<Value> {
    let response = builder
        .send()
        .await
        .map_err(|error| Error::unavailable(format!("Meegle could not be reached: {error}")))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);

    envelope(status.is_success(), status.as_u16(), &parsed, &text)
}

fn envelope(success: bool, status: u16, parsed: &Value, text: &str) -> Result<Value> {
    let code = parsed["err_code"].as_i64().unwrap_or(0);

    if !success || code != 0 {
        let message = parsed["err_msg"]
            .as_str()
            .filter(|message| !message.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| text.chars().take(300).collect());

        return Err(Error::unavailable(format!(
            "Meegle answered {status} (code {code}): {message}"
        )));
    }

    Ok(parsed["data"].clone())
}

fn work_item_id(id: &str) -> Result<i64> {
    id.trim()
        .parse()
        .map_err(|_| Error::invalid(format!("`{id}` is not a Meegle work item ID")))
}

fn state_of(data: &Value, done_states: &[String]) -> TicketState {
    let item = data
        .as_array()
        .and_then(|items| items.first())
        .unwrap_or(data);
    let status = &item["work_item_status"];
    let key = status["state_key"].as_str().unwrap_or_default();

    TicketState {
        status: status["name"]
            .as_str()
            .filter(|name| !name.is_empty())
            .unwrap_or(key)
            .to_string(),
        done: !key.is_empty() && done_states.iter().any(|done| done == key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_code_fails_even_with_a_200() {
        let parsed = json!({ "err_code": 10022, "err_msg": "token expired", "data": null });

        let error = envelope(true, 200, &parsed, "").unwrap_err().to_string();
        assert!(error.contains("10022"));
        assert!(error.contains("token expired"));

        let fine = json!({ "err_code": 0, "err_msg": "", "data": 123 });
        assert_eq!(envelope(true, 200, &fine, "").unwrap(), json!(123));
    }

    #[test]
    fn a_work_item_is_done_in_one_of_the_states_named_done() {
        let data =
            json!([{ "id": 1, "work_item_status": { "state_key": "CLOSED", "name": "已关闭" } }]);

        let state = state_of(&data, &["CLOSED".into()]);
        assert!(state.done);
        assert_eq!(state.status, "已关闭");
        assert!(!state_of(&data, &[]).done);
    }

    #[test]
    fn a_token_is_renewed_three_minutes_early() {
        let token = PluginToken {
            token: "t".into(),
            expires_at: 1_000,
        };

        assert!(token.fresh(800));
        assert!(!token.fresh(830));
    }
}
