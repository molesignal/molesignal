// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 和发起这一轮的人说话：把任务清单、问题与确认卡写进对话，等回答。
//!
//! 等的时候 Run 的状态是 `waiting_input` / `waiting_approval`，让出执行名额；回答到了（同一节点
//! 直接提醒，别的节点经 NOTIFY 转来，没有提醒时隔几秒自己查）再拿回名额接着执行。最多等
//! [`RESPONSE_TIMEOUT`]，等不到就告诉模型没有回答。

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::time::Instant;

use super::{
    super::super::tool_dispatcher::{Reply, RunInteraction},
    RunControl,
};
use crate::{
    agent::{
        model::RiskLevel,
        runs::{NewRunEvent, Pending, QuestionKind, RunEventKind, RunResponse},
    },
    shared::ids::Id,
};

/// 最多等提问人这么久。
pub(super) const RESPONSE_TIMEOUT: Duration = Duration::from_secs(60 * 60);
/// 没有提醒时多久自己查一次回答。
const POLL: Duration = Duration::from_secs(5);

pub(super) struct Interaction {
    control: Arc<RunControl>,
}

enum Waited {
    Answered(RunResponse),
    TimedOut,
    Stopped,
}

impl Interaction {
    pub(super) fn new(control: Arc<RunControl>) -> Self {
        Self { control }
    }

    async fn wait(&self, pending: Pending, kind: RunEventKind) -> Waited {
        let control = &self.control;
        let runs = &control.state.agent.runs;
        let run_id = &control.run.id;

        // 先订阅再记下在等什么，回答来得再快也不会漏掉提醒。
        let mut signals = runs.signals();

        if let Err(error) = runs.wait_for(run_id, &pending).await {
            tracing::debug!(run_id = %run_id.0, %error, "the run stopped before it could wait");
            return Waited::Stopped;
        }

        control
            .emit_all(vec![
                agent_event(kind, &pending),
                agent_event(
                    RunEventKind::Status,
                    &json!({ "status": pending.status().as_str() }),
                ),
            ])
            .await;

        control.slot.release();

        let deadline = Instant::now() + RESPONSE_TIMEOUT;

        let waited = loop {
            if control.cancel.is_cancelled() {
                break Waited::Stopped;
            }

            match runs.take_response(run_id).await {
                Ok(Some(response)) => break Waited::Answered(response),
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(run_id = %run_id.0, %error, "the answer to a question could not be read")
                }
            }

            let now = Instant::now();

            if now >= deadline {
                let _ = runs.stop_waiting(run_id).await;
                break Waited::TimedOut;
            }

            tokio::select! {
                _ = control.cancel.cancelled() => {}
                _ = signals.changed(run_id, (deadline - now).min(POLL)) => {}
            }
        };

        control.slot.reacquire(&control.cancel).await;

        if let Waited::Answered(response) = &waited {
            control
                .emit_all(vec![
                    agent_event(RunEventKind::Response, response),
                    agent_event(RunEventKind::Status, &json!({ "status": "running" })),
                ])
                .await;
        } else if matches!(waited, Waited::TimedOut) {
            control
                .emit_all(vec![
                    agent_event(
                        RunEventKind::Notice,
                        &json!({ "kind": "no_response", "call_id": pending.call_id() }),
                    ),
                    agent_event(RunEventKind::Status, &json!({ "status": "running" })),
                ])
                .await;
        }

        waited
    }
}

fn agent_event(kind: RunEventKind, payload: &impl serde::Serialize) -> NewRunEvent {
    NewRunEvent::new(kind, serde_json::to_value(payload).unwrap_or(Value::Null))
}

#[async_trait]
impl RunInteraction for Interaction {
    async fn todo(&self, items: Value) {
        self.control
            .emit(RunEventKind::Todo, json!({ "items": items }))
            .await;
    }

    async fn ask(&self, question: String, kind: QuestionKind, options: Vec<String>) -> Reply {
        let pending = Pending::Question {
            call_id: Id::new().0,
            question,
            kind,
            options,
        };

        match self.wait(pending, RunEventKind::Question).await {
            Waited::Answered(RunResponse::Answer { choices, text, .. }) => {
                Reply::Answered { choices, text }
            }
            Waited::Answered(RunResponse::Decision { .. }) | Waited::TimedOut => Reply::TimedOut,
            Waited::Stopped => Reply::Stopped,
        }
    }

    async fn notice(&self, payload: Value) {
        self.control.emit(RunEventKind::Notice, payload).await;
    }

    async fn confirm(&self, tool: &str, arguments: &Value, risk: RiskLevel) -> Reply {
        let pending = Pending::Confirmation {
            call_id: Id::new().0,
            tool: tool.to_string(),
            arguments: arguments.clone(),
            risk,
        };

        match self.wait(pending, RunEventKind::Confirmation).await {
            Waited::Answered(RunResponse::Decision { approved: true, .. }) => Reply::Approved,
            Waited::Answered(RunResponse::Decision {
                approved: false, ..
            }) => Reply::Declined,
            Waited::Answered(RunResponse::Answer { .. }) | Waited::TimedOut => Reply::TimedOut,
            Waited::Stopped => Reply::Stopped,
        }
    }
}
