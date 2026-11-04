// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 作战室：用组织选定的、能建群的消息连接器建群、发消息：飞书 App（`lark_app`）、Slack App
//! （`slack_app`）、钉钉企业内部应用（`dingtalk_app`）和企业微信自建应用（`wecom_app`）。飞书 App 还能
//! 预约飞书视频会议。飞书与 Slack 只连连接器地址解析出的 IP，不跟随跳转。
//!
//! - 飞书：`auth/v3/tenant_access_token/internal` 换 Token；`im/v1/chats` 建群（成员是 open_id），
//!   `im/v1/chats/{chat_id}/link` 取入群链接；`im/v1/messages` 发消息；`vc/v1/reserves/apply` 预约会议。
//! - Slack：`conversations.create` 建频道，`conversations.invite` 拉人，`chat.postMessage` 发消息。
//! - 钉钉：`v1.0/im/group/create` 建内部群（成员是 userId），机器人发群消息；机器人要在钉钉里加进群。
//! - 企业微信：`appchat/create` 建应用群聊（成员是 userid，至少两人），`appchat/send` 发消息。
//!
//! 钉钉和企业微信的群没有入群链接，成员在客户端里直接看到它。

use std::time::Duration;

use reqwest::header::USER_AGENT;
use serde_json::{Value, json};

use crate::{
    domain::{alerting::response::WarRoom, notify::connector::NotifyConnector},
    infra::{
        connectors::dispatcher::http::pinned_client,
        notify::adapters::{
            DINGTALK_APP_CONNECTOR_TYPE, DingTalkApp, DingTalkRecipient, WECOM_APP_CONNECTOR_TYPE,
            create_wecom_chat, post_wecom_chat,
        },
    },
    shared::{Error, Result, time::TimestampMicros},
};

const TIMEOUT: Duration = Duration::from_secs(15);
const FEISHU_API: &str = "https://open.feishu.cn/open-apis";
const SLACK_API: &str = "https://slack.com/api";

pub const LARK_APP: &str = "lark_app";
pub const SLACK_APP: &str = "slack_app";

/// 能建作战室的消息连接器：它们的接口能用连接器已有的凭证建群、拉人、发消息。
pub const WAR_ROOM_TYPES: [&str; 4] = [
    LARK_APP,
    SLACK_APP,
    DINGTALK_APP_CONNECTOR_TYPE,
    WECOM_APP_CONNECTOR_TYPE,
];

fn config<'a>(connector: &'a NotifyConnector, key: &str) -> Option<&'a str> {
    connector
        .config
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn api_base(connector: &NotifyConnector, fallback: &str) -> String {
    config(connector, "api_base_url")
        .unwrap_or(fallback)
        .trim_end_matches('/')
        .to_string()
}

async fn client(base: &str) -> Result<reqwest::Client> {
    let url =
        url::Url::parse(base).map_err(|error| Error::invalid(format!("API address: {error}")))?;
    pinned_client(&url, TIMEOUT).await
}

/// 建群，把 `members`（连接器里的身份：飞书 open_id、Slack 用户 ID、钉钉 userId、企业微信 userid）拉进来，
/// `owner` 当群主（钉钉和企业微信要），发一句开场白。
pub async fn create(
    connector: &NotifyConnector,
    name: &str,
    owner: Option<&str>,
    members: &[String],
    intro: &str,
) -> Result<WarRoom> {
    let owner = owner.or_else(|| members.first().map(String::as_str));

    let (chat_id, url) = match connector.connector_type.as_str() {
        LARK_APP => feishu_create(connector, name, members).await?,
        SLACK_APP => slack_create(connector, name, members).await?,
        DINGTALK_APP_CONNECTOR_TYPE => {
            let owner = owner.ok_or_else(|| Error::invalid("a DingTalk group needs an owner"))?;
            let app = DingTalkApp::parse(&connector.config)?;
            let id = app.create_group(&app.http()?, name, owner, members).await?;

            (id, String::new())
        }
        WECOM_APP_CONNECTOR_TYPE => {
            let id = create_wecom_chat(&connector.config, name, owner, members).await?;

            (id, String::new())
        }
        other => {
            return Err(Error::invalid(format!(
                "a war room is made with a connector that can make group chats, not `{other}`"
            )));
        }
    };

    if let Err(error) = post(connector, &chat_id, intro).await {
        tracing::warn!(%error, "the war room's first message could not be sent");
    }

    Ok(WarRoom {
        platform: connector.connector_type.clone(),
        connector_id: connector.id.clone(),
        chat_id,
        url,
    })
}

/// 往作战室发一段文字。
pub async fn post(connector: &NotifyConnector, chat_id: &str, text: &str) -> Result<()> {
    match connector.connector_type.as_str() {
        LARK_APP => {
            let base = api_base(connector, FEISHU_API);
            let http = client(&base).await?;
            let token = feishu_token(&http, connector, &base).await?;

            feishu(
                http.post(format!("{base}/im/v1/messages?receive_id_type=chat_id"))
                    .bearer_auth(&token)
                    .json(&json!({
                        "receive_id": chat_id,
                        "msg_type": "text",
                        "content": json!({ "text": text }).to_string(),
                    })),
            )
            .await?;

            Ok(())
        }
        SLACK_APP => {
            let base = api_base(connector, SLACK_API);
            let http = client(&base).await?;

            slack(
                http.post(format!("{base}/chat.postMessage"))
                    .bearer_auth(slack_token(connector)?)
                    .json(&json!({ "channel": chat_id, "text": text })),
            )
            .await?;

            Ok(())
        }
        DINGTALK_APP_CONNECTOR_TYPE => {
            let app = DingTalkApp::parse(&connector.config)?;
            let title = text.lines().next().unwrap_or("MoleSignal");

            app.send(
                &app.http()?,
                DingTalkRecipient::Conversation(chat_id),
                title,
                text,
            )
            .await
            .map(|_| ())
        }
        WECOM_APP_CONNECTOR_TYPE => post_wecom_chat(&connector.config, chat_id, text).await,
        _ => Ok(()),
    }
}

/// 用飞书 App 预约一场飞书视频会议，返回会议链接；`owner` 是会议所有人的 open_id。
pub async fn reserve_meeting(
    connector: &NotifyConnector,
    topic: &str,
    owner: Option<&str>,
) -> Result<String> {
    if connector.connector_type != LARK_APP {
        return Err(Error::invalid(
            "a Feishu meeting is booked through a Feishu App connector",
        ));
    }

    let base = api_base(connector, FEISHU_API);
    let http = client(&base).await?;
    let token = feishu_token(&http, connector, &base).await?;
    let end = TimestampMicros::now().0 / 1_000_000 + super::meetings::MEETING_SECS;

    let mut body = json!({
        "end_time": end.to_string(),
        "meeting_settings": { "topic": topic.chars().take(100).collect::<String>() },
    });

    if let Some(owner) = owner {
        body["owner_id"] = json!(owner);
    }

    let data = feishu(
        http.post(format!("{base}/vc/v1/reserves/apply?user_id_type=open_id"))
            .bearer_auth(&token)
            .json(&body),
    )
    .await?;

    data["reserve"]["url"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| Error::unavailable("Feishu did not return the meeting link"))
}

async fn feishu_token(
    http: &reqwest::Client,
    connector: &NotifyConnector,
    base: &str,
) -> Result<String> {
    let (Some(app_id), Some(app_secret)) =
        (config(connector, "app_id"), config(connector, "app_secret"))
    else {
        return Err(Error::invalid(
            "the Feishu App connector has no app ID or secret",
        ));
    };

    let response = http
        .post(format!("{base}/auth/v3/tenant_access_token/internal"))
        .header(USER_AGENT, "MoleSignal")
        .json(&json!({ "app_id": app_id, "app_secret": app_secret }))
        .send()
        .await
        .map_err(|error| Error::unavailable(format!("Feishu could not be reached: {error}")))?;
    let body: Value = response.json().await.unwrap_or(Value::Null);

    if body["code"].as_i64().unwrap_or(-1) != 0 {
        return Err(Error::unavailable(format!(
            "Feishu refused the app credentials: {}",
            body["msg"].as_str().unwrap_or_default()
        )));
    }

    body["tenant_access_token"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| Error::unavailable("Feishu did not issue a token"))
}

/// 飞书的回答：`code` 为 0 时读出 `data`。
async fn feishu(builder: reqwest::RequestBuilder) -> Result<Value> {
    let response = builder
        .header(USER_AGENT, "MoleSignal")
        .send()
        .await
        .map_err(|error| Error::unavailable(format!("Feishu could not be reached: {error}")))?;
    let body: Value = response.json().await.unwrap_or(Value::Null);

    if body["code"].as_i64().unwrap_or(-1) != 0 {
        return Err(Error::unavailable(format!(
            "Feishu answered {}: {}",
            body["code"],
            body["msg"].as_str().unwrap_or_default()
        )));
    }

    Ok(body["data"].clone())
}

async fn feishu_create(
    connector: &NotifyConnector,
    name: &str,
    members: &[String],
) -> Result<(String, String)> {
    let base = api_base(connector, FEISHU_API);
    let http = client(&base).await?;
    let token = feishu_token(&http, connector, &base).await?;

    let created = feishu(
        http.post(format!("{base}/im/v1/chats?user_id_type=open_id"))
            .bearer_auth(&token)
            .json(&json!({
                "name": name.chars().take(60).collect::<String>(),
                "chat_mode": "group",
                "chat_type": "private",
                "user_id_list": members,
            })),
    )
    .await?;

    let chat_id = created["chat_id"]
        .as_str()
        .ok_or_else(|| Error::unavailable("Feishu did not return the chat ID"))?
        .to_string();

    let link = feishu(
        http.post(format!("{base}/im/v1/chats/{chat_id}/link"))
            .bearer_auth(&token)
            .json(&json!({ "validity_period": "permanently" })),
    )
    .await
    .ok()
    .and_then(|data| data["share_link"].as_str().map(str::to_string))
    .unwrap_or_default();

    Ok((chat_id, link))
}

fn slack_token(connector: &NotifyConnector) -> Result<&str> {
    config(connector, "bot_token")
        .ok_or_else(|| Error::invalid("the Slack App connector has no bot token"))
}

/// Slack 的回答：`ok` 为真时原样返回。
async fn slack(builder: reqwest::RequestBuilder) -> Result<Value> {
    let response = builder
        .header(USER_AGENT, "MoleSignal")
        .send()
        .await
        .map_err(|error| Error::unavailable(format!("Slack could not be reached: {error}")))?;
    let body: Value = response.json().await.unwrap_or(Value::Null);

    if body["ok"].as_bool() != Some(true) {
        return Err(Error::unavailable(format!(
            "Slack answered: {}",
            body["error"].as_str().unwrap_or("unknown error")
        )));
    }

    Ok(body)
}

async fn slack_create(
    connector: &NotifyConnector,
    name: &str,
    members: &[String],
) -> Result<(String, String)> {
    let base = api_base(connector, SLACK_API);
    let http = client(&base).await?;
    let token = slack_token(connector)?;

    let created = slack(
        http.post(format!("{base}/conversations.create"))
            .bearer_auth(token)
            .json(&json!({ "name": slack_channel_name(name), "is_private": false })),
    )
    .await?;

    let channel = created["channel"]["id"]
        .as_str()
        .ok_or_else(|| Error::unavailable("Slack did not return the channel ID"))?
        .to_string();

    if !members.is_empty()
        && let Err(error) = slack(
            http.post(format!("{base}/conversations.invite"))
                .bearer_auth(token)
                .json(&json!({ "channel": channel, "users": members.join(",") })),
        )
        .await
    {
        tracing::warn!(%error, "people could not be invited to the war room");
    }

    let url = format!("https://slack.com/app_redirect?channel={channel}");
    Ok((channel, url))
}

/// Slack 频道名：小写字母、数字、`-` 和 `_`，最长 80。
pub fn slack_channel_name(name: &str) -> String {
    let mut out = String::new();

    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }

    let trimmed = out.trim_matches('-');
    let name = if trimmed.is_empty() {
        "incident"
    } else {
        trimmed
    };

    name.chars().take(80).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slack_channel_names_are_lowercase_and_dashed() {
        assert_eq!(
            slack_channel_name("INC 2292 · Checkout p95 > 800ms!"),
            "inc-2292-checkout-p95-800ms"
        );
        assert_eq!(slack_channel_name("支付故障"), "incident");
        assert!(slack_channel_name(&"x".repeat(200)).len() <= 80);
    }
}
