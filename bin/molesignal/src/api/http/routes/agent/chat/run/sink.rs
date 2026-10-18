// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! AgentLoop 的事件 → Run 的事件。
//!
//! 模型写出的文字攒一会儿再写（[`TextBuffer`]），其他事件之前先把攒着的写掉。工具结果在事件里
//! 只留开头；完整结果进证据（太大时进对象存储），审计只记摘要。同时记下这一轮走过的步骤
//! （[`TranscriptBuilder`]），回答落库时一起存。

use std::{collections::HashMap, sync::Arc, time::Instant};

use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::{Value, json};

use super::{super::evidence::build_evidence, RunControl};
use crate::{
    agent::{
        chat::{AgentSink, AgentStreamEvent},
        runs::{RunEventKind, TextBuffer, TranscriptBuilder, TranscriptStep, inline_result},
    },
    api::http::routes::activity_audit,
    app::iam::IamContext,
    shared::ids::Id,
};

#[derive(Default)]
struct Gathered {
    buffer: TextBuffer,
    transcript: TranscriptBuilder,
    /// tool_call_id → (名称, 参数)
    tools: HashMap<String, (String, Value)>,
    evidence: Vec<Value>,
}

/// 一轮结束时留下的东西。
pub(super) struct Record {
    pub(super) steps: Vec<TranscriptStep>,
    /// 最后一次模型调用写的文字：被停下时是写到一半的回答。
    pub(super) pending_text: String,
    pub(super) evidence: Vec<Value>,
}

pub(super) struct RunSink {
    control: Arc<RunControl>,
    ctx: IamContext,
    chat_id: Id,
    /// 这一轮回答落库时的 ID：证据的对象 key 用它。
    message_id: Id,
    gathered: Mutex<Gathered>,
}

impl RunSink {
    pub(super) fn new(
        control: Arc<RunControl>,
        ctx: IamContext,
        chat_id: Id,
        message_id: Id,
    ) -> Self {
        Self {
            control,
            ctx,
            chat_id,
            message_id,
            gathered: Mutex::new(Gathered::default()),
        }
    }

    async fn write_text(&self, text: String) {
        if !text.is_empty() {
            self.control
                .emit(RunEventKind::Text, json!({ "text": text }))
                .await;
        }
    }

    /// 攒得够久的文字写掉。
    pub(super) async fn flush_if_due(&self) {
        let due = {
            let mut gathered = self.gathered.lock();
            gathered
                .buffer
                .due(Instant::now())
                .then(|| gathered.buffer.take())
        };

        if let Some(text) = due {
            self.write_text(text).await;
        }
    }

    pub(super) async fn flush(&self) {
        let text = self.gathered.lock().buffer.take();
        self.write_text(text).await;
    }

    pub(super) fn into_record(self) -> Record {
        let gathered = self.gathered.into_inner();

        Record {
            pending_text: gathered.transcript.pending_text().to_string(),
            steps: gathered.transcript.into_steps(),
            evidence: gathered.evidence,
        }
    }

    async fn tool_end(&self, id: String, result: String, is_error: bool) {
        let (name, arguments) = {
            let mut gathered = self.gathered.lock();
            gathered.transcript.end(&id, &result, is_error);

            gathered
                .tools
                .get(&id)
                .cloned()
                .unwrap_or_else(|| ("unknown".into(), Value::Null))
        };

        let state = &self.control.state;

        let entry = build_evidence(
            state,
            &self.ctx.org_id,
            &self.chat_id,
            &self.message_id,
            &id,
            &name,
            arguments,
            &result,
            is_error,
        )
        .await;

        // 审计只记录摘要，不写原始可观测数据。
        activity_audit::record(
            state,
            &self.ctx,
            "agent.tool.called",
            "agent_chat",
            &self.chat_id.0,
            json!({
                "tool": name,
                "run_id": self.control.run.id.0,
                "status": if is_error { "error" } else { "success" },
                "row_count": entry.get("row_count").cloned().unwrap_or(Value::Null),
                "object_key": entry.get("object_key").cloned().unwrap_or(Value::Null),
            }),
        )
        .await;

        self.gathered.lock().evidence.push(entry);

        let (kept, truncated) = inline_result(&result);

        self.control
            .emit(
                RunEventKind::ToolEnd,
                json!({
                    "id": id,
                    "name": name,
                    "result": kept,
                    "truncated": truncated,
                    "is_error": is_error,
                }),
            )
            .await;
    }
}

#[async_trait]
impl AgentSink for RunSink {
    async fn emit(&self, event: AgentStreamEvent) {
        match event {
            AgentStreamEvent::Delta(text) => {
                let full = {
                    let mut gathered = self.gathered.lock();
                    gathered.transcript.text(&text);
                    gathered.buffer.push(&text)
                };

                if let Some(text) = full {
                    self.write_text(text).await;
                }
            }
            AgentStreamEvent::ToolStart {
                id,
                name,
                arguments,
            } => {
                let parsed: Value =
                    serde_json::from_str(&arguments).unwrap_or(Value::String(arguments));

                let pending = {
                    let mut gathered = self.gathered.lock();
                    gathered.transcript.start(&id, &name, parsed.clone());
                    gathered
                        .tools
                        .insert(id.clone(), (name.clone(), parsed.clone()));
                    gathered.buffer.take()
                };

                self.write_text(pending).await;

                self.control
                    .emit(
                        RunEventKind::ToolStart,
                        json!({ "id": id, "name": name, "arguments": parsed }),
                    )
                    .await;
            }
            AgentStreamEvent::ToolEnd {
                id,
                result,
                is_error,
            } => self.tool_end(id, result, is_error).await,
            // 最终回答随结束事件一起写；这里只把还攒着的文字写掉。
            AgentStreamEvent::Chunk(_) | AgentStreamEvent::Done { .. } => self.flush().await,
            AgentStreamEvent::Error(_) => self.flush().await,
        }
    }

    fn is_cancelled(&self) -> bool {
        self.control.cancel.is_cancelled()
    }

    async fn cancelled(&self) {
        self.control.cancel.cancelled().await;
    }
}
