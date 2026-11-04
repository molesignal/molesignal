// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 企业微信智能机器人：长连接收消息，回复也走这条连接。
//!
//! 用 Bot ID 与 Secret 订阅（`aibot_subscribe`），每 30 秒 `ping` 一次。发给机器人的消息是
//! `aibot_msg_callback`；回复（`aibot_respond_msg`）带上它的 `req_id`，24 小时内有效。机器人
//! 主动发的消息是 `aibot_send_msg`，对方要先在这个会话里给机器人发过消息。
//!
//! 一个机器人同一时间只有一条连接，新的订阅会把旧的挤掉（旧的收到 `disconnected_event`）。
//! 所以回复只能从连着的节点发：本节点连着时直接写进连接（见 [`holds`]），别的节点把回复记进
//! 待发的回复，由连着的节点发出去。
//!
//! 「处理中」是一条流式消息：排了回答就发一帧没结束的流，回答好了用同一个流 ID 发结束帧，
//! 原地换成回答。流从第一帧起 10 分钟内必须结束，超过 9 分钟才好的回答另发一条消息。
//! 企业微信的群里没有话题：一个人在一个群里的提问是一个会话。
//!
//! 告警通知与卡片上按钮的点击见 [`cards`]。

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::{
    sync::{mpsc, oneshot},
    time::{Instant, interval_at},
};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

pub(super) use self::cards::answer;
use self::cards::card_action;
pub(crate) use self::cards::{deliver, post_notification};
use super::{Listener, ws};
use crate::{
    agent::im::{ConversationKind, Inbound, Sender},
    infra::notify::adapters::{WecomBot, truncate_bytes},
    shared::{Error, Result, ids::Id},
};

const PING: Duration = Duration::from_secs(30);
/// 订阅与每一帧都等企业微信回话这么久。
const ANSWER_TIMEOUT: Duration = Duration::from_secs(15);
/// 流式消息从第一帧起 10 分钟内要结束；留一分钟余量。
const STREAM_WINDOW_MS: i64 = 9 * 60 * 1000;
/// Markdown 与流式消息的正文最多这么多字节。
const MAX_CONTENT_BYTES: usize = 20_480;

/// 要写进连接的一帧，和等它回话的一方。
struct Outgoing {
    frame: Value,
    done: oneshot::Sender<Result<()>>,
}

/// 本节点连着的一个机器人。`serial` 区分前后两条连接：旧连接退出时不删新连接的入口。
struct Held {
    serial: u64,
    tx: mpsc::Sender<Outgoing>,
}

/// Bot ID → 本节点连着的那条连接。
static HELD: LazyLock<Mutex<HashMap<String, Held>>> = LazyLock::new(Default::default);
static SERIAL: AtomicU64 = AtomicU64::new(0);

/// 连接活着时登记在 [`HELD`] 里，结束时撤掉自己这一条。
struct Registration {
    bot_id: String,
    serial: u64,
}

impl Registration {
    fn new(bot_id: &str, tx: mpsc::Sender<Outgoing>) -> Self {
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);

        if let Ok(mut held) = HELD.lock() {
            held.insert(bot_id.to_string(), Held { serial, tx });
        }

        Self {
            bot_id: bot_id.to_string(),
            serial,
        }
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        if let Ok(mut held) = HELD.lock()
            && held
                .get(&self.bot_id)
                .is_some_and(|held| held.serial == self.serial)
        {
            held.remove(&self.bot_id);
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

fn frame(cmd: &str, req_id: &str, body: Value) -> Value {
    json!({ "cmd": cmd, "headers": { "req_id": req_id }, "body": body })
}

fn lost(error: impl std::fmt::Display) -> Error {
    Error::unavailable(format!("WeCom connection lost: {error}"))
}

/// 企业微信对一帧的回话：`errcode` 是 0 才算成了。
fn answered(frame: &Value) -> Result<()> {
    match frame["errcode"].as_i64().unwrap_or_default() {
        0 => Ok(()),
        code => Err(Error::unavailable(format!(
            "WeCom refused it: {} ({code})",
            frame["errmsg"].as_str().unwrap_or("unknown error")
        ))),
    }
}

/// 本节点连着这个机器人：回复可以直接发。
pub(crate) fn holds(config: &Value) -> bool {
    let Ok(bot) = WecomBot::parse(config) else {
        return false;
    };

    HELD.lock()
        .is_ok_and(|held| held.contains_key(bot.bot_id.trim()))
}

/// 把一帧写进本节点连着的连接，等企业微信回话。
async fn send(config: &Value, frame: Value) -> Result<()> {
    let bot = WecomBot::parse(config)?;
    let tx = HELD
        .lock()
        .ok()
        .and_then(|held| held.get(bot.bot_id.trim()).map(|held| held.tx.clone()))
        .ok_or_else(|| {
            Error::unavailable("this server does not hold the WeCom smart bot's connection")
        })?;

    let (done, answer) = oneshot::channel();
    tx.send(Outgoing { frame, done })
        .await
        .map_err(|_| Error::unavailable("the WeCom connection closed"))?;

    match tokio::time::timeout(ANSWER_TIMEOUT, answer).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(Error::unavailable("the WeCom connection closed")),
        Err(_) => Err(Error::unavailable("WeCom did not answer in time")),
    }
}

async fn write(socket: &mut ws::Socket, frame: &Value) -> Result<()> {
    socket
        .send(Message::Text(frame.to_string().into()))
        .await
        .map_err(lost)
}

/// 订阅，等它的回话；别的帧先放过。
async fn subscribe(socket: &mut ws::Socket, bot: &WecomBot) -> Result<()> {
    let id = Id::new().0;
    let request = frame(
        "aibot_subscribe",
        &id,
        json!({ "bot_id": bot.bot_id.trim(), "secret": bot.secret() }),
    );
    write(socket, &request).await?;

    let wait = async {
        while let Some(message) = socket.next().await {
            if let Message::Text(text) = message.map_err(lost)?
                && let Ok(frame) = serde_json::from_str::<Value>(&text)
                && frame["headers"]["req_id"] == id.as_str()
            {
                return answered(&frame).map_err(|error| {
                    Error::unavailable(format!(
                        "WeCom refused the smart bot's subscription: {error}"
                    ))
                });
            }
        }

        Err(Error::unavailable("WeCom closed the connection"))
    };

    tokio::time::timeout(ANSWER_TIMEOUT, wait)
        .await
        .map_err(|_| Error::unavailable("WeCom did not answer the subscription"))?
}

pub(super) async fn listen(
    config: &Value,
    listener: &dyn Listener,
    cancel: &CancellationToken,
) -> Result<()> {
    let bot = WecomBot::parse(config)?;
    let mut socket = ws::connect(bot.ws_url()).await?;
    subscribe(&mut socket, &bot).await?;

    let (tx, mut outgoing) = mpsc::channel::<Outgoing>(64);
    let _registration = Registration::new(bot.bot_id.trim(), tx);
    listener.connected(bot.bot_id.trim()).await;

    // 写出去还没回话的帧，按 req_id 排队（同一条消息的流式帧共用一个 req_id）。
    let mut waiting: HashMap<String, VecDeque<oneshot::Sender<Result<()>>>> = HashMap::new();
    let mut ticker = interval_at(Instant::now() + PING, PING);
    let mut heard = Instant::now();

    loop {
        tokio::select! {
            () = cancel.cancelled() => {
                let _ = socket.close(None).await;
                return Ok(());
            }
            _ = ticker.tick() => {
                if heard.elapsed() > PING * 3 {
                    return Err(Error::unavailable("WeCom stopped answering"));
                }

                // 等不及走了的不再等。
                waiting.retain(|_, queue| {
                    queue.retain(|done| !done.is_closed());
                    !queue.is_empty()
                });

                let ping = json!({ "cmd": "ping", "headers": { "req_id": Id::new().0 } });
                write(&mut socket, &ping).await?;
            }
            Some(request) = outgoing.recv() => {
                let id = request.frame["headers"]["req_id"].as_str().unwrap_or_default().to_string();

                if let Err(error) = write(&mut socket, &request.frame).await {
                    let _ = request.done.send(Err(Error::unavailable("the WeCom connection closed")));
                    return Err(error);
                }

                waiting.entry(id).or_default().push_back(request.done);
            }
            message = socket.next() => {
                let Some(message) = message else {
                    return Ok(());
                };

                heard = Instant::now();

                let frame: Value = match message.map_err(lost)? {
                    Message::Text(text) => match serde_json::from_str(&text) {
                        Ok(frame) => frame,
                        Err(_) => continue,
                    },
                    Message::Close(_) => return Ok(()),
                    _ => continue,
                };

                match frame["cmd"].as_str() {
                    Some("aibot_msg_callback") => {
                        if let Some(inbound) = inbound(&frame, now_ms()) {
                            listener.message(inbound).await;
                        }
                    }
                    Some("aibot_event_callback") => {
                        if frame["body"]["event"]["eventtype"] == "disconnected_event" {
                            return Err(Error::unavailable(
                                "another connection subscribed this smart bot and took it over",
                            ));
                        }

                        if let Some(action) = card_action(&frame) {
                            listener.action(action).await;
                        }
                    }
                    // 回话：交给最早写出这个 req_id 的一方。
                    _ if frame.get("errcode").is_some() => {
                        let id = frame["headers"]["req_id"].as_str().unwrap_or_default();

                        if let Some(queue) = waiting.get_mut(id) {
                            if let Some(done) = queue.pop_front() {
                                let _ = done.send(answered(&frame));
                            }

                            if queue.is_empty() {
                                waiting.remove(id);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

/// 群里的消息以 @ 机器人开头：去掉它。企业微信在 @ 的名字后面放一个四分之一全角空格，
/// 没有时去掉第一个词。
fn strip_mention(text: &str) -> &str {
    let text = text.trim();

    let Some(rest) = text.strip_prefix('@') else {
        return text;
    };

    let end = rest
        .find('\u{2005}')
        .map(|at| at + '\u{2005}'.len_utf8())
        .or_else(|| rest.find(char::is_whitespace))
        .unwrap_or(rest.len());

    rest[end..].trim()
}

/// 发给机器人的一条消息。只认文字：文本、图文混排里的文字、语音转成的文字。
pub(crate) fn inbound(frame: &Value, received_at_ms: i64) -> Option<Inbound> {
    let body = &frame["body"];
    let req_id = frame["headers"]["req_id"].as_str()?;
    let message_id = body["msgid"].as_str()?.to_string();
    let user = body["from"]["userid"]
        .as_str()
        .filter(|user| !user.is_empty())?
        .to_string();
    let group = body["chattype"].as_str() == Some("group");

    let text = match body["msgtype"].as_str()? {
        "text" => body["text"]["content"].as_str()?.to_string(),
        "voice" => body["voice"]["content"].as_str()?.to_string(),
        "mixed" => body["mixed"]["msg_item"]
            .as_array()?
            .iter()
            .filter(|item| item["msgtype"] == "text")
            .filter_map(|item| item["text"]["content"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };

    let (conversation, text) = if group {
        let chat = body["chatid"].as_str()?.to_string();
        (chat, strip_mention(&text).to_string())
    } else {
        (user.clone(), text.trim().to_string())
    };

    Some(Inbound {
        message_id,
        conversation_id: conversation.clone(),
        kind: if group {
            ConversationKind::Group
        } else {
            ConversationKind::Direct
        },
        thread_key: if group {
            format!("user:{user}")
        } else {
            String::new()
        },
        sender: Sender {
            ids: vec![user],
            name: None,
        },
        text,
        addressed: true,
        reply: json!({
            "req_id": req_id,
            "chat_id": conversation,
            "chat_type": if group { 2 } else { 1 },
            "received_at": received_at_ms,
        }),
    })
}

fn req_id(reply: &Value) -> Result<&str> {
    reply["req_id"]
        .as_str()
        .ok_or_else(|| Error::invalid("the WeCom message to answer is unknown"))
}

fn markdown_body(markdown: &str) -> Value {
    json!({
        "msgtype": "markdown",
        "markdown": { "content": truncate_bytes(markdown, MAX_CONTENT_BYTES) },
    })
}

fn stream_body(stream: &str, finish: bool, content: &str) -> Value {
    json!({
        "msgtype": "stream",
        "stream": {
            "id": stream,
            "finish": finish,
            "content": truncate_bytes(content, MAX_CONTENT_BYTES),
        },
    })
}

/// 回复到原处：答复这条消息。
pub(super) async fn reply(config: &Value, reply: &Value, markdown: &str) -> Result<()> {
    let body = markdown_body(markdown);
    send(config, frame("aibot_respond_msg", req_id(reply)?, body)).await
}

/// 「处理中」：一帧没结束的流。返回流 ID 与开始的时间。
pub(super) async fn react(config: &Value, reply: &Value, working: &str) -> Result<String> {
    let stream = Id::new().0;
    let body = stream_body(&stream, false, working);
    send(config, frame("aibot_respond_msg", req_id(reply)?, body)).await?;

    Ok(format!("{stream}@{}", now_ms()))
}

/// 一条「处理中」的流：流 ID 与开始的时间。
fn working_stream(working: &str) -> Option<(&str, i64)> {
    let (stream, started) = working.rsplit_once('@')?;
    Some((stream, started.parse().ok()?))
}

/// 把「处理中」的流换成回答；流快到时限了就另发一条。
pub(super) async fn finish(
    config: &Value,
    reply: &Value,
    working: Option<&str>,
    markdown: &str,
) -> Result<()> {
    match working.and_then(working_stream) {
        Some((stream, started)) if now_ms() - started < STREAM_WINDOW_MS => {
            let body = stream_body(stream, true, markdown);
            send(config, frame("aibot_respond_msg", req_id(reply)?, body)).await
        }
        Some(_) => {
            let chat = reply["chat_id"].as_str().unwrap_or_default();
            let kind = reply["chat_type"].as_i64().unwrap_or(1);
            post(config, chat, kind, markdown).await
        }
        None => self::reply(config, reply, markdown).await,
    }
}

/// 机器人主动发一条消息：`kind` 1 是单聊（`chat` 是 userid），2 是群。
async fn post(config: &Value, chat: &str, kind: i64, markdown: &str) -> Result<()> {
    let mut body = markdown_body(markdown);
    body["chatid"] = json!(chat);
    body["chat_type"] = json!(kind);

    send(config, frame("aibot_send_msg", &Id::new().0, body)).await
}

/// 单聊发给一个人：他要先私聊过机器人。
pub(super) async fn tell(config: &Value, user_id: &str, markdown: &str) -> Result<()> {
    post(config, user_id, 1, markdown).await
}

mod cards;
#[cfg(test)]
mod tests;
