// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 模型调用写进 `_agent_model_traces` 之前先排进一个有界队列，由后台 worker 成批写入。
//!
//! 写入走 intake，而写入管道里的 LLM 函数自己也要记用量：直接在调用路径上写会让 intake
//! 等待它自己。排队还让对话不必等这条记录写完。队列满时丢弃并记一条日志，用量计数不受影响。

use serde_json::{Map, Value, json};
use tokio::sync::mpsc;

use super::UsageScope;
use crate::{
    agent::{chat::ModelCall, usage::Cost},
    shared::{ids::Id, time::TimestampMicros},
};

/// 排队等写入的记录最多这么多条。
const QUEUE_CAPACITY: usize = 1024;

/// 失败原因最多保留这么多字符。
const ERROR_CHARS: usize = 500;

/// 一条等写入的模型调用记录。
#[derive(Debug, Clone)]
pub struct TraceEvent {
    pub org_id: Id,
    pub at: TimestampMicros,
    pub fields: Map<String, Value>,
}

#[derive(Clone)]
pub struct TraceQueue {
    sender: mpsc::Sender<TraceEvent>,
}

pub type TraceReceiver = mpsc::Receiver<TraceEvent>;

/// 一对队列两端：发送端交给用量服务，接收端交给写入 worker。
pub fn trace_queue() -> (TraceQueue, TraceReceiver) {
    let (sender, receiver) = mpsc::channel(QUEUE_CAPACITY);

    (TraceQueue { sender }, receiver)
}

impl TraceQueue {
    pub fn push(&self, event: TraceEvent) {
        if let Err(error) = self.sender.try_send(event) {
            tracing::debug!(%error, "agent model trace dropped");
        }
    }
}

/// 一次调用在 `_agent_model_traces` 里的字段。字段名不带点：SQL 里点是限定符。
pub(super) fn trace_event(scope: &UsageScope, call: &ModelCall, cost: Option<Cost>) -> TraceEvent {
    let mut fields = Map::new();

    fields.insert("origin".into(), json!(scope.origin.as_str()));
    fields.insert("provider".into(), json!(call.provider.as_str()));
    fields.insert("model".into(), json!(call.model));
    fields.insert("prompt_tokens".into(), json!(call.prompt_tokens));
    fields.insert(
        "cached_prompt_tokens".into(),
        json!(call.cached_prompt_tokens),
    );
    fields.insert("completion_tokens".into(), json!(call.completion_tokens));
    fields.insert("total_tokens".into(), json!(call.total_tokens()));
    fields.insert("latency_ms".into(), json!(call.latency_ms));
    fields.insert("streaming".into(), json!(call.streaming));
    fields.insert("tool_calls".into(), json!(call.tool_calls));
    fields.insert(
        "status".into(),
        json!(if call.succeeded() { "ok" } else { "error" }),
    );
    fields.insert("gen_ai_system".into(), json!(call.provider.as_str()));
    fields.insert("gen_ai_request_model".into(), json!(call.model));

    let optional = [
        ("user_id", scope.user_id.as_ref().map(|id| json!(id.0))),
        ("chat_id", scope.chat_id.as_ref().map(|id| json!(id.0))),
        (
            "provider_id",
            scope.provider_id.as_ref().map(|id| json!(id.0)),
        ),
        (
            "profile_id",
            scope.profile_id.as_ref().map(|id| json!(id.0)),
        ),
        (
            "prompt_hash",
            scope.prompt_hash.as_ref().map(|hash| json!(hash)),
        ),
        ("skill_id", scope.skill_id.as_ref().map(|id| json!(id))),
        (
            "finish_reason",
            call.finish_reason.as_ref().map(|reason| json!(reason)),
        ),
        ("cost", cost.map(|cost| json!(cost.amount))),
        (
            "cost_currency",
            cost.map(|cost| json!(cost.currency.as_str())),
        ),
        (
            "error",
            call.error
                .as_ref()
                .map(|error| json!(error.chars().take(ERROR_CHARS).collect::<String>())),
        ),
    ];

    for (name, value) in optional {
        if let Some(value) = value {
            fields.insert(name.into(), value);
        }
    }

    TraceEvent {
        org_id: scope.org_id.clone(),
        at: TimestampMicros::now(),
        fields,
    }
}
