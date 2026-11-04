// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 飞书：长连接收「接收消息」事件，开放接口回复。
//!
//! 长连接与飞书 SDK 一样：用 App ID 与 App Secret 换一个 `wss://` 地址；帧是 protobuf 的
//! `Frame`，控制帧做心跳（ping 带地址里的 `service_id`，pong 带新的配置），数据帧是事件，可能
//! 分片，拼齐后处理并原样回一帧，载荷是 `{"code":200}`。机器人只在群里被 @ 或私聊时收到消息
//! （有「获取群组中所有消息」权限时群里的都会收到，没 @ 的只在已有话题里回答）。
//!
//! 「卡片回传交互」（`card.action.trigger`）也从长连接来：点了告警卡片上的按钮，回的那一帧在
//! `data` 里带着结果（base64 的 JSON：一个提示与换上的卡片），飞书等 3 秒。

use std::{collections::HashMap, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures::{SinkExt, StreamExt};
use prost::Message as _;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::time::{Instant, interval_at};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::{Listener, ws};
use crate::{
    agent::im::CardAnswer,
    infra::notify::adapters::{LARK_SITES, TOKENS, TokenCache, lark_card},
    shared::{
        Error, Result,
        http_trace::{self, HttpTarget},
    },
};

const PING_DEFAULT_SECS: u64 = 120;
/// 收到的回复与卡片最多这么多字符。
const MAX_MARKDOWN_CHARS: usize = 20_000;
/// 「处理中」的表情。
const WORKING_EMOJI: &str = "OnIt";

const FRAME_CONTROL: i32 = 0;
const FRAME_DATA: i32 = 1;

#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct Header {
    #[prost(string, required, tag = "1")]
    pub key: String,
    #[prost(string, required, tag = "2")]
    pub value: String,
}

#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct Frame {
    #[prost(uint64, required, tag = "1")]
    pub seq_id: u64,
    #[prost(uint64, required, tag = "2")]
    pub log_id: u64,
    #[prost(int32, required, tag = "3")]
    pub service: i32,
    #[prost(int32, required, tag = "4")]
    pub method: i32,
    #[prost(message, repeated, tag = "5")]
    pub headers: Vec<Header>,
    #[prost(string, optional, tag = "6")]
    pub payload_encoding: Option<String>,
    #[prost(string, optional, tag = "7")]
    pub payload_type: Option<String>,
    #[prost(bytes = "vec", optional, tag = "8")]
    pub payload: Option<Vec<u8>>,
    #[prost(string, optional, tag = "9")]
    pub log_id_new: Option<String>,
}

impl Frame {
    fn header(&self, key: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|header| header.key == key)
            .map(|header| header.value.as_str())
    }

    fn ping(service: i32) -> Self {
        Self {
            seq_id: 0,
            log_id: 0,
            service,
            method: FRAME_CONTROL,
            headers: vec![Header {
                key: "type".into(),
                value: "ping".into(),
            }],
            payload_encoding: None,
            payload_type: None,
            payload: None,
            log_id_new: None,
        }
    }
}

fn default_api_base_url() -> String {
    LARK_SITES[0].1.into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct App {
    app_id: String,
    app_secret: String,
    #[serde(default = "default_api_base_url")]
    api_base_url: String,
}

impl App {
    fn parse(config: &Value) -> Result<Self> {
        let app: Self = serde_json::from_value(config.clone())
            .map_err(|error| Error::invalid(format!("invalid lark_app config: {error}")))?;

        if app.app_id.trim().is_empty() || app.app_secret.trim().is_empty() {
            return Err(Error::invalid(
                "lark_app app_id and app_secret cannot be empty",
            ));
        }

        Ok(app)
    }

    fn api(&self) -> &str {
        self.api_base_url.trim().trim_end_matches('/')
    }

    /// 长连接的地址在开放平台的根上，不在 `/open-apis` 下。
    fn domain(&self) -> &str {
        self.api().trim_end_matches("/open-apis")
    }

    async fn token(&self, http: &reqwest::Client) -> Result<String> {
        let key = TokenCache::key("lark_app", &self.app_id, &self.app_secret);

        TOKENS
            .get(&key, async {
                let body: Value = send(
                    http,
                    http.post(format!(
                        "{}/auth/v3/tenant_access_token/internal",
                        self.api()
                    ))
                    .json(&json!({
                        "app_id": self.app_id.trim(),
                        "app_secret": self.app_secret.trim(),
                    })),
                )
                .await?;

                let token = body["tenant_access_token"].as_str().unwrap_or_default();

                if body["code"].as_i64() != Some(0) || token.is_empty() {
                    return Err(Error::unavailable(format!(
                        "Feishu refused the app credentials: {}",
                        body["msg"].as_str().unwrap_or("unknown_error")
                    )));
                }

                Ok((token.to_string(), body["expire"].as_u64().unwrap_or(7_200)))
            })
            .await
    }
}

fn http() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| Error::internal("Feishu HTTP client could not be initialized"))
}

async fn send(http: &reqwest::Client, request: reqwest::RequestBuilder) -> Result<Value> {
    let response = http_trace::send(http, request, HttpTarget::ThirdParty)
        .await
        .map_err(|error| Error::unavailable(format!("Feishu request failed: {error}")))?;

    response
        .json()
        .await
        .map_err(|_| Error::unavailable("Feishu returned an invalid response"))
}

/// 开放接口的返回：`code` 不是 0 时说明原因。
fn ok(body: Value, action: &str) -> Result<Value> {
    if body["code"].as_i64() == Some(0) {
        Ok(body)
    } else {
        Err(Error::unavailable(format!(
            "Feishu refused {action}: {}",
            body["msg"].as_str().unwrap_or("unknown_error")
        )))
    }
}

/// 长连接的地址与心跳间隔。
async fn endpoint(http: &reqwest::Client, app: &App) -> Result<(String, u64)> {
    let body = send(
        http,
        http.post(format!("{}/callback/ws/endpoint", app.domain()))
            .header("locale", "zh")
            .json(&json!({ "AppID": app.app_id.trim(), "AppSecret": app.app_secret.trim() })),
    )
    .await?;

    let body = ok(body, "the long connection")?;
    let url = body["data"]["URL"]
        .as_str()
        .filter(|url| !url.is_empty())
        .ok_or_else(|| Error::unavailable("Feishu returned no long connection address"))?;

    let ping = body["data"]["ClientConfig"]["PingInterval"]
        .as_u64()
        .filter(|seconds| *seconds > 0)
        .unwrap_or(PING_DEFAULT_SECS);

    Ok((url.to_string(), ping))
}

pub(super) async fn listen(
    config: &Value,
    listener: &dyn Listener,
    cancel: &CancellationToken,
) -> Result<()> {
    let app = App::parse(config)?;
    let http = http()?;

    let token = app.token(&http).await?;
    let bot = ok(
        send(
            &http,
            http.get(format!("{}/bot/v3/info", app.api()))
                .bearer_auth(&token),
        )
        .await?,
        "reading the bot",
    )?;
    let bot_open_id = bot["bot"]["open_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let bot_name = bot["bot"]["app_name"]
        .as_str()
        .unwrap_or_default()
        .to_string();

    let (url, ping_secs) = endpoint(&http, &app).await?;
    let service = Url::parse(&url)
        .ok()
        .and_then(|url| {
            url.query_pairs()
                .find(|(key, _)| key == "service_id")
                .and_then(|(_, value)| value.parse::<i32>().ok())
        })
        .unwrap_or(0);

    let mut socket = ws::connect(&url).await?;
    listener.connected(&bot_name).await;

    let mut ping = Duration::from_secs(ping_secs);
    let mut ticker = interval_at(Instant::now(), ping);
    let mut heard = Instant::now();
    let mut chunks: HashMap<String, Vec<Option<Vec<u8>>>> = HashMap::new();

    loop {
        tokio::select! {
            () = cancel.cancelled() => {
                let _ = socket.close(None).await;
                return Ok(());
            }
            _ = ticker.tick() => {
                if heard.elapsed() > ping * 3 {
                    return Err(Error::unavailable("Feishu stopped answering the heartbeat"));
                }

                socket
                    .send(Message::Binary(Frame::ping(service).encode_to_vec().into()))
                    .await
                    .map_err(|error| Error::unavailable(format!("Feishu connection lost: {error}")))?;
            }
            message = socket.next() => {
                let Some(message) = message else {
                    return Ok(());
                };

                let message = message
                    .map_err(|error| Error::unavailable(format!("Feishu connection lost: {error}")))?;
                heard = Instant::now();

                let frame = match message {
                    Message::Binary(bytes) => match Frame::decode(bytes.as_ref()) {
                        Ok(frame) => frame,
                        Err(error) => {
                            tracing::debug!(%error, "an undecodable Feishu frame was skipped");
                            continue;
                        }
                    },
                    Message::Close(_) => return Ok(()),
                    _ => continue,
                };

                if frame.method == FRAME_CONTROL {
                    if let Some(next) = pong_interval(&frame) && next != ping {
                        ping = next;
                        ticker = interval_at(Instant::now() + ping, ping);
                    }

                    continue;
                }

                if frame.method != FRAME_DATA {
                    continue;
                }

                let started = Instant::now();
                let Some(payload) = assemble(&mut chunks, &frame) else {
                    continue;
                };

                let mut answer = None;

                if matches!(frame.header("type"), Some("event" | "card"))
                    && let Ok(event) = serde_json::from_slice::<Value>(&payload)
                {
                    match event["header"]["event_type"].as_str() {
                        Some("im.message.receive_v1") => {
                            if let Some(inbound) = inbound(&event["event"], &bot_open_id) {
                                listener.message(inbound).await;
                            }
                        }
                        Some("card.action.trigger") => {
                            if let Some(action) = card_action(&event["event"]) {
                                answer = listener.action(action).await;
                            }
                        }
                        _ => {}
                    }
                }

                let reply = acknowledge(frame, started.elapsed(), answer.as_ref());

                socket
                    .send(Message::Binary(reply.encode_to_vec().into()))
                    .await
                    .map_err(|error| Error::unavailable(format!("Feishu connection lost: {error}")))?;
            }
        }
    }
}

/// pong 带来的新心跳间隔。
fn pong_interval(frame: &Frame) -> Option<Duration> {
    if frame.header("type") != Some("pong") {
        return None;
    }

    let config: Value = serde_json::from_slice(frame.payload.as_deref()?).ok()?;

    config["PingInterval"]
        .as_u64()
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
}

/// 拼齐分片的载荷；还缺分片时返回 `None`。
fn assemble(chunks: &mut HashMap<String, Vec<Option<Vec<u8>>>>, frame: &Frame) -> Option<Vec<u8>> {
    let payload = frame.payload.clone().unwrap_or_default();
    let sum = frame
        .header("sum")
        .and_then(|sum| sum.parse::<usize>().ok())
        .unwrap_or(1);

    if sum <= 1 {
        return Some(payload);
    }

    let seq = frame
        .header("seq")
        .and_then(|seq| seq.parse::<usize>().ok())?;
    let id = frame.header("message_id")?.to_string();

    // 太多没拼齐的消息说明有分片丢了，丢掉旧的。
    if chunks.len() > 64 {
        chunks.clear();
    }

    let parts = chunks.entry(id.clone()).or_insert_with(|| vec![None; sum]);

    if seq >= parts.len() {
        return None;
    }

    parts[seq] = Some(payload);

    if parts.iter().any(Option::is_none) {
        return None;
    }

    chunks
        .remove(&id)
        .map(|parts| parts.into_iter().flatten().flatten().collect())
}

/// 处理完一个数据帧后回给飞书的帧：原帧带上处理用时，载荷是成功；点了卡片按钮的带上结果。
fn acknowledge(mut frame: Frame, elapsed: Duration, answer: Option<&CardAnswer>) -> Frame {
    frame.headers.push(Header {
        key: "biz_rt".into(),
        value: elapsed.as_millis().to_string(),
    });

    let data = answer.map(|answer| BASE64.encode(card_response(answer).to_string()));

    frame.payload = Some(
        json!({ "code": 200, "headers": {}, "data": data })
            .to_string()
            .into_bytes(),
    );
    frame
}

/// 点击的结果写成「卡片回传交互」的响应：给点的人的提示与换上的卡片。
fn card_response(answer: &CardAnswer) -> Value {
    let mut response = json!({});

    if let Some((kind, text)) = &answer.notice {
        response["toast"] = json!({ "type": kind.as_str(), "content": text });
    }

    if let Some(card) = &answer.card {
        response["card"] = json!({
            "type": "raw",
            "data": lark_card(&card.message, &card.buttons, card.note.as_deref()),
        });
    }

    response
}

mod message;
mod rest;

#[cfg(test)]
use message::post_text;
pub(crate) use message::{card_action, inbound};
pub(super) use rest::{answer, react, reply, tell, unreact};

#[cfg(test)]
mod tests;
