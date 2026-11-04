// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一次运行结束：写回运行记录，按输出发通知、写事件的 RCA。
//!
//! Run 在执行它的节点上结束时直接调 [`finished`]；节点没了、Run 被标为中断，或写回时出错，由
//! worker 的 [`reconcile`] 补上。运行记录只结束一次；通知按运行与渠道去重，RCA 是覆盖写，
//! 两边都做到也不会重复。

use serde::Serialize;
use serde_json::json;

use crate::{
    agent::{
        answer::conclusion,
        automations::{Automation, AutomationRun, AutomationRunStatus, RunEnding, reasons},
        runs::{Run, RunStatus},
    },
    api::{
        AppState,
        http::routes::agent::{budget, investigate::answer_as_rca},
    },
    app::{
        automations::outcome::{
            self, AnswerEnding, KEY_PREFIX, Notice, RECORDED_SUMMARY_CHARS, excerpt,
        },
        notify::DirectSend,
    },
    domain::{
        alerting::incident::IncidentRca,
        notify::connector::{NotifyTarget, NotifyTargetType},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 一轮 Run 结束后要的东西：执行它的节点在写下 Run 的结局之前记下。
#[derive(Debug, Clone)]
pub(crate) struct AnswerEnd {
    pub(crate) status: RunStatus,
    pub(crate) code: Option<String>,
    /// 成功时的回答。
    pub(crate) content: Option<String>,
    pub(crate) timed_out: bool,
    /// 没有人可问时被挡住的地方。
    pub(crate) blocked: Vec<&'static str>,
}

/// 一个渠道的投递结果，记在运行记录上。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Delivery {
    connector_id: Id,
    target_type: NotifyTargetType,
    target_masked: String,
    delivered: bool,
    error: Option<String>,
}

/// Run 结束时调：写回运行记录与输出。出错只记日志，worker 的收尾会再试。
pub(crate) async fn finished(state: &AppState, run: &Run, end: AnswerEnd) {
    if let Err(error) = settle(state, run, end).await {
        tracing::warn!(run_id = %run.id.0, %error, "an automation run could not be finished");
    }
}

async fn settle(state: &AppState, run: &Run, end: AnswerEnd) -> Result<()> {
    let Some(record) = state.agent.automations.run_for_agent_run(&run.id).await? else {
        return Ok(());
    };

    if record.status.is_over() {
        return Ok(());
    }

    // 对话用的预算代码换成运行记录的原因代码。
    let code = end.code.map(|code| {
        if code == budget::BUDGET_EXHAUSTED {
            reasons::BUDGET_EXHAUSTED.to_string()
        } else {
            code
        }
    });

    let ending = AnswerEnding {
        status: end.status,
        code,
        timed_out: end.timed_out,
    };
    let (status, found) = outcome::status_of(&ending, &end.blocked);

    // 规则在运行期间被删掉了：记录随规则一起删，什么都不用发。
    let automation = match state
        .agent
        .automations
        .get(&record.org_id, &record.automation_id)
        .await
    {
        Ok(automation) => automation,
        Err(Error::NotFound(_)) => return Ok(()),
        Err(error) => return Err(error),
    };

    let content = end.content.as_deref();
    let mut deliveries = Vec::new();
    let mut rca = None;

    if outcome::notifies(status) && !automation.outputs.channels.is_empty() {
        deliveries = notify(state, &automation, &record, status, &found, content).await?;
    }

    let answered = matches!(
        status,
        AutomationRunStatus::Succeeded | AutomationRunStatus::Blocked
    );

    if automation.outputs.write_rca
        && answered
        && let (Some(incident_id), Some(content)) = (&record.incident_id, content)
    {
        rca = Some(write_rca(state, &record, incident_id, content).await);
    }

    let delivery_failed = deliveries.iter().any(|delivery| !delivery.delivered);
    let rca_failed = rca == Some(false);
    let (status, found) = outcome::after_outputs((status, found), delivery_failed, rca_failed);

    let ending = RunEnding {
        status,
        reason: (!found.is_empty()).then(|| found.join(",")),
        summary: content.map(|content| excerpt(&conclusion(content), RECORDED_SUMMARY_CHARS)),
        outputs: json!({ "deliveries": deliveries, "rca": rca }),
    };

    state
        .agent
        .automations
        .finish_run(&record.id, &ending)
        .await?;
    Ok(())
}

async fn notify(
    state: &AppState,
    automation: &Automation,
    record: &AutomationRun,
    status: AutomationRunStatus,
    found: &[&'static str],
    content: Option<&str>,
) -> Result<Vec<Delivery>> {
    let pack = state
        .platform
        .i18n_language
        .load_pack(KEY_PREFIX, &automation.language)
        .await?;

    let url = format!(
        "{}/agent/chat?chat={}",
        state.platform.external_url.trim_end_matches('/'),
        record
            .chat_id
            .as_ref()
            .map(|id| id.0.as_str())
            .unwrap_or_default()
    );

    // 通知里只放回答的结论：回答本身是给页面看的 JSON。
    let answer = content.map(conclusion);

    let message = outcome::notification(
        &pack,
        &Notice {
            name: &automation.name,
            status,
            reasons: found,
            answer: answer.as_deref(),
            url: &url,
        },
    )?;

    let mut deliveries = Vec::new();

    for channel in &automation.outputs.channels {
        let send = DirectSend {
            connector_id: channel.connector_id.clone(),
            target: NotifyTarget {
                target_type: channel.target_type,
                value: channel.target.clone(),
                metadata: Default::default(),
            },
            message: message.clone(),
            event_id: format!("automation.run:{}", record.id.0),
            // 同一次运行对同一个渠道只发一次。
            idempotency_key: format!(
                "automation:{}:{}:{}:{}",
                record.id.0,
                channel.connector_id.0,
                channel.target_type.as_str(),
                blake3::hash(channel.target.as_bytes()).to_hex()
            ),
        };

        let outcome = state
            .alerting
            .notify
            .send_direct(&record.org_id, send)
            .await;

        deliveries.push(Delivery {
            connector_id: channel.connector_id.clone(),
            target_type: channel.target_type,
            target_masked: outcome.target_masked,
            delivered: outcome.delivered,
            error: outcome.error,
        });
    }

    Ok(deliveries)
}

/// 把回答写成事件的 RCA。有人从对话里写过的 RCA 保留不动。
async fn write_rca(
    state: &AppState,
    record: &AutomationRun,
    incident_id: &Id,
    content: &str,
) -> bool {
    let written = async {
        if let Some(existing) = state.agent.incident_rca.get(incident_id).await?
            && existing.finish_reason.as_deref() == Some("chat")
        {
            return Ok(());
        }

        let chat = match &record.chat_id {
            Some(chat_id) => state
                .agent
                .chats
                .get_chat(&record.org_id, chat_id)
                .await
                .ok(),
            None => None,
        };
        let language = chat
            .as_ref()
            .map(|chat| chat.reply_language.as_str())
            .unwrap_or_default();
        let summary = answer_as_rca(state, language, content).await;
        let now = TimestampMicros::now();

        state
            .agent
            .incident_rca
            .upsert(IncidentRca {
                incident_id: incident_id.clone(),
                org_id: record.org_id.clone(),
                summary,
                provider: chat.as_ref().map(|chat| chat.provider.clone()),
                model: chat.as_ref().map(|chat| chat.model.clone()),
                prompt_builtin_key: None,
                prompt_hash: None,
                prompt_tokens: 0,
                completion_tokens: 0,
                finish_reason: Some("automation".into()),
                created_at: now,
                updated_at: now,
            })
            .await
            .map(|_| ())
    };

    match written.await {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(run = %record.id.0, %error, "an automation could not write an incident's RCA");
            false
        }
    }
}

/// 运行记录还在运行、Run 却已经结束超过一分钟：执行它的节点没了，或写回时出错。照 Run 的
/// 结局补上。还没排上 Run 就停在那里超过十分钟的，记为失败。
pub(crate) async fn reconcile(state: &AppState) -> Result<usize> {
    const SETTLED_AFTER: i64 = 60_000_000;
    const NEVER_QUEUED_AFTER: i64 = 600_000_000;

    let now = TimestampMicros::now();
    let mut settled = 0;

    for record in state.agent.automations.running_runs(100).await? {
        let Some(run_id) = &record.run_id else {
            if now.0 - record.created_at.0 > NEVER_QUEUED_AFTER {
                let ending = RunEnding {
                    status: AutomationRunStatus::Failed,
                    reason: Some(reasons::ERROR.into()),
                    summary: None,
                    outputs: json!({}),
                };
                state
                    .agent
                    .automations
                    .finish_run(&record.id, &ending)
                    .await?;
                settled += 1;
            }
            continue;
        };

        let run = match state.agent.runs.get(&record.org_id, run_id).await {
            Ok(run) => run,
            Err(Error::NotFound(_)) => {
                let ending = RunEnding {
                    status: AutomationRunStatus::Abandoned,
                    reason: Some(reasons::INTERRUPTED.into()),
                    summary: None,
                    outputs: json!({}),
                };
                state
                    .agent
                    .automations
                    .finish_run(&record.id, &ending)
                    .await?;
                settled += 1;
                continue;
            }
            Err(error) => return Err(error),
        };

        let over_for_a_while = run
            .finished_at
            .is_some_and(|finished| now.0 - finished.0 > SETTLED_AFTER);

        if !run.status.is_terminal() || !over_for_a_while {
            continue;
        }

        let content = match (&record.chat_id, &run.assistant_message_id) {
            (Some(chat_id), Some(message_id)) if run.status == RunStatus::Succeeded => state
                .agent
                .chats
                .list_messages(chat_id)
                .await?
                .into_iter()
                .find(|message| message.id == *message_id)
                .map(|message| message.content),
            _ => None,
        };

        let end = AnswerEnd {
            status: run.status,
            code: run
                .error
                .clone()
                .filter(|error| error == budget::BUDGET_EXHAUSTED),
            content,
            timed_out: false,
            blocked: Vec::new(),
        };

        settle(state, &run, end).await?;
        settled += 1;
    }

    Ok(settled)
}
