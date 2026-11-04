// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一轮 IM 回答结束：把回答（或没答成的原因）回复到原处，换掉「处理中」。
//!
//! 执行 Run 的节点结束时直接调 [`finished`]；节点没了、Run 被标为中断，或结束时还没记下 Run
//! 与消息的对应，由 worker 的 [`reconcile`] 补上。回复只发一次：先把消息从「等回答」换成
//! 「已回复」，换到了才发。回复只能从连着机器人的节点发时（企业微信），本节点不连着就把回复
//! 记进待发的回复，由连着的节点在 [`drain`] 里发出去。

use super::texts::Texts;
use crate::{
    agent::{
        im::{ImMessage, ImOutgoing, ImPlatform, MessageStatus},
        runs::{Run, RunSource, RunStatus},
    },
    api::{
        AppState,
        http::routes::agent::{automations::AnswerEnd, budget, investigate::answer_as_rca},
    },
    domain::notify::connector::NotifyConnector,
    infra::im::{self, wecom},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 等回答的消息过了这么久才由 worker 补回复：给执行 Run 的节点留出自己回复的时间。
const RECONCILE_AFTER_MICROS: i64 = 60 * 1_000_000;
const RECONCILE_BATCH: i64 = 50;
/// 连着的节点一次发多少条别的节点交来的回复。
const DRAIN_BATCH: i64 = 20;
/// 待发的回复过了这么久就不发了：企业微信只能在收到消息后 24 小时内回复。
const OUTGOING_LIFETIME_MICROS: i64 = 23 * 60 * 60 * 1_000_000;

/// Run 结束时调：是 IM 里的提问就回复。出错只记日志，worker 会补上。
pub(crate) async fn finished(state: &AppState, run: &Run, end: Option<&AnswerEnd>) {
    if run.source != RunSource::Im {
        return;
    }

    let sent = async {
        let Some(message) = state.agent.im.message_for_run(&run.id).await? else {
            return Ok(());
        };

        deliver(state, message, run, end).await
    };

    if let Err(error) = sent.await {
        tracing::warn!(run_id = %run.id.0, %error, "an IM answer could not be sent");
    }
}

/// 补上结束了还没回复的；返回补了几条。
pub(crate) async fn reconcile(state: &AppState) -> Result<usize> {
    let before = TimestampMicros(TimestampMicros::now().0 - RECONCILE_AFTER_MICROS);
    let mut sent = 0;

    for message in state.agent.im.answering(before, RECONCILE_BATCH).await? {
        let Some(run_id) = message.run_id.clone() else {
            continue;
        };

        let run = match state.agent.runs.get(&message.org_id, &run_id).await {
            Ok(run) => run,
            Err(Error::NotFound(_)) => {
                let mut message = message;
                message.status = MessageStatus::Failed;
                message.detail = Some("the run is gone".into());
                state.agent.im.update_message(&message).await?;
                continue;
            }
            Err(error) => return Err(error),
        };

        if !run.status.is_terminal() {
            continue;
        }

        deliver(state, message, &run, None).await?;
        sent += 1;
    }

    Ok(sent)
}

/// 回答的正文：Run 写下的那条回答。
async fn written_answer(state: &AppState, run: &Run) -> Option<String> {
    let id = run.assistant_message_id.as_ref()?;

    state
        .agent
        .chats
        .list_messages(&run.chat_id)
        .await
        .ok()?
        .into_iter()
        .find(|message| &message.id == id)
        .map(|message| message.content)
}

/// 会话在 MoleSignal 里的链接；没配对外地址时没有。
fn chat_link(state: &AppState, texts: &Texts, run: &Run) -> Option<String> {
    let base = state.platform.external_url.trim().trim_end_matches('/');

    (!base.is_empty()).then(|| {
        format!(
            "[{}]({base}/agent/chat?chat={})",
            texts.get("open", &[]),
            run.chat_id.0
        )
    })
}

/// 回复的正文：回答（结构化的排成 Markdown），或没答成的原因，最后是会话的链接。
async fn compose(
    state: &AppState,
    texts: &Texts,
    run: &Run,
    end: Option<&AnswerEnd>,
    content: Option<String>,
) -> String {
    let content = content.filter(|content| !content.trim().is_empty());
    let timed_out = end.is_some_and(|end| end.timed_out);
    let budget = end.and_then(|end| end.code.as_deref()) == Some(budget::BUDGET_EXHAUSTED);

    let mut parts = vec![match (run.status, content) {
        (RunStatus::Succeeded, Some(content)) => {
            answer_as_rca(state, &texts.language, &content).await
        }
        _ if timed_out => texts.get("timed_out", &[]),
        (RunStatus::Cancelled, _) => texts.get("stopped", &[]),
        _ if budget => texts.get("budget", &[]),
        _ => texts.get(
            "failed",
            &[(
                "reason",
                run.error.as_deref().unwrap_or(run.status.as_str()),
            )],
        ),
    }];

    if end.is_some_and(|end| !end.blocked.is_empty()) {
        parts.push(texts.get("blocked", &[]));
    }

    if let Some(link) = chat_link(state, texts, run) {
        parts.push(link);
    }

    parts.join("\n\n")
}

async fn deliver(
    state: &AppState,
    mut message: ImMessage,
    run: &Run,
    end: Option<&AnswerEnd>,
) -> Result<()> {
    if !state
        .agent
        .im
        .move_message(
            &message.id,
            MessageStatus::Answering,
            MessageStatus::Replied,
        )
        .await?
    {
        return Ok(());
    }

    let connector = state
        .alerting
        .notify
        .get_connector(&message.org_id, &message.connector_id)
        .await?;
    let Some(platform) = ImPlatform::of_connector(&connector.connector_type) else {
        return Ok(());
    };

    let texts = Texts::for_user(state, &message.org_id, message.user_id.as_ref()).await;
    let content = match end.and_then(|end| end.content.clone()) {
        Some(content) => Some(content),
        None => written_answer(state, run).await,
    };
    let markdown = compose(state, &texts, run, end, content).await;

    // 本节点发不出这个机器人的回复：交给连着它的节点。
    if !im::holds(platform, &connector.config) {
        return state
            .agent
            .im
            .enqueue_outgoing(&ImOutgoing {
                id: Id::new(),
                org_id: message.org_id.clone(),
                connector_id: message.connector_id.clone(),
                message_id: Some(message.id.clone()),
                reply: message.reply.clone(),
                working: message.reaction_id.clone(),
                markdown,
                notification: None,
                created_at: TimestampMicros::now(),
            })
            .await;
    }

    let sent = im::finish(
        platform,
        &connector.config,
        &message.reply,
        message.reaction_id.as_deref(),
        &markdown,
    )
    .await;

    message.replied_at = Some(TimestampMicros::now());
    (message.status, message.detail) = match sent {
        Ok(()) => (MessageStatus::Replied, None),
        Err(error) => (MessageStatus::Failed, Some(error.to_string())),
    };

    state.agent.im.update_message(&message).await
}

/// 连着机器人的节点发出别的节点交来的回复与通知（只有企业微信智能机器人的通知走这里）；返回发了
/// 几条。连接断了时剩下的放回去，等连上再发。
pub(crate) async fn drain(state: &AppState, connector: &NotifyConnector) -> Result<usize> {
    let Some(platform) = ImPlatform::of_connector(&connector.connector_type) else {
        return Ok(0);
    };

    if !im::holds(platform, &connector.config) {
        return Ok(0);
    }

    let mut sent = 0;

    for outgoing in state
        .agent
        .im
        .take_outgoing(&connector.id, DRAIN_BATCH)
        .await?
    {
        if !im::holds(platform, &connector.config) {
            state.agent.im.enqueue_outgoing(&outgoing).await?;
            continue;
        }

        let result = if TimestampMicros::now().0 - outgoing.created_at.0 > OUTGOING_LIFETIME_MICROS
        {
            Err(Error::unavailable(
                "the reply waited too long for the bot's connection",
            ))
        } else if let Some(notification) = &outgoing.notification {
            wecom::post_notification(&connector.config, notification).await
        } else {
            im::finish(
                platform,
                &connector.config,
                &outgoing.reply,
                outgoing.working.as_deref(),
                &outgoing.markdown,
            )
            .await
        };

        let (status, detail) = match &result {
            Ok(()) => (MessageStatus::Replied, None),
            Err(error) => (MessageStatus::Failed, Some(error.to_string())),
        };

        if let Some(message_id) = &outgoing.message_id {
            state
                .agent
                .im
                .settle_message(
                    message_id,
                    status,
                    detail.as_deref(),
                    TimestampMicros::now(),
                )
                .await?;
        }

        sent += usize::from(result.is_ok());
    }

    Ok(sent)
}
