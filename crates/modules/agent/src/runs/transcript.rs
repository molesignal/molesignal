// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一轮回答里模型走过的步骤：每一步是模型写的一段说明和它要调的工具，连同工具结果。
//!
//! 回答存下时一起存（`agent_messages.tool_transcript`）；下一轮交给模型时还原成带工具调用的
//! assistant 消息与工具结果，追问时模型看得到上一轮查到的数据。结果超过
//! [`MAX_INLINE_RESULT_BYTES`] 的只留开头，完整结果在证据里。

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    agent::chat::{ChatMessage, MessageRole},
    shared::{ids::Id, time::TimestampMicros},
};

/// 一个工具结果在历史里最多留这么多字节。
pub const MAX_INLINE_RESULT_BYTES: usize = 16 * 1024;

/// 没等到结果的调用（被停下）在历史里的结果。
const UNFINISHED_RESULT: &str = "[the run was stopped before this call returned]";

/// 一次工具调用。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    /// 结果文本，太长时截断；还没有结果时为空。
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub is_error: bool,
}

/// 模型的一次调用：先写的说明（可以为空），和它要调的工具。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TranscriptStep {
    #[serde(default)]
    pub text: String,
    pub calls: Vec<TranscriptCall>,
}

/// 结果在历史里留的样子：太长时留开头，并说明截断了。
pub fn inline_result(result: &str) -> (String, bool) {
    if result.len() <= MAX_INLINE_RESULT_BYTES {
        return (result.to_string(), false);
    }

    let mut end = MAX_INLINE_RESULT_BYTES;

    while !result.is_char_boundary(end) {
        end -= 1;
    }

    (
        format!(
            "{}\n[truncated: the result was {} bytes; call the tool again for the rest]",
            &result[..end],
            result.len()
        ),
        true,
    )
}

/// 按流式事件记下一轮回答的步骤。
#[derive(Debug, Default)]
pub struct TranscriptBuilder {
    steps: Vec<TranscriptStep>,
    /// 这次模型调用写到现在的文字。
    text: String,
}

impl TranscriptBuilder {
    pub fn text(&mut self, delta: &str) {
        self.text.push_str(delta);
    }

    /// 一次工具调用开始。同一次模型调用的工具调用都在任何结果之前到：前一步已经有结果，
    /// 或者这之前又写了文字，就是新的一步。
    pub fn start(&mut self, id: &str, name: &str, arguments: Value) {
        let new_step = !self.text.is_empty()
            || self
                .steps
                .last()
                .is_none_or(|step| step.calls.iter().any(|call| call.result.is_some()));

        if new_step {
            self.steps.push(TranscriptStep {
                text: std::mem::take(&mut self.text),
                calls: Vec::new(),
            });
        }

        if let Some(step) = self.steps.last_mut() {
            step.calls.push(TranscriptCall {
                id: id.to_string(),
                name: name.to_string(),
                arguments,
                result: None,
                truncated: false,
                is_error: false,
            });
        }
    }

    pub fn end(&mut self, id: &str, result: &str, is_error: bool) {
        let call = self
            .steps
            .iter_mut()
            .rev()
            .flat_map(|step| step.calls.iter_mut())
            .find(|call| call.id == id);

        if let Some(call) = call {
            let (kept, truncated) = inline_result(result);

            call.result = Some(kept);
            call.truncated = truncated;
            call.is_error = is_error;
        }
    }

    /// 最后一次模型调用写的文字：回答，或被停下时写到一半的文字。
    pub fn pending_text(&self) -> &str {
        &self.text
    }

    pub fn calls(&self) -> usize {
        self.steps.iter().map(|step| step.calls.len()).sum()
    }

    pub fn into_steps(self) -> Vec<TranscriptStep> {
        self.steps
    }
}

/// 还原成交给模型的消息：每一步一条带工具调用的 assistant 消息，后面跟各个工具结果。
pub fn transcript_messages(
    chat_id: &Id,
    steps: &[TranscriptStep],
    at: TimestampMicros,
) -> Vec<ChatMessage> {
    let message = |role, content: String| ChatMessage {
        id: Id::new(),
        chat_id: chat_id.clone(),
        role,
        content,
        tool_call_id: None,
        tool_calls: Value::Null,
        created_at: at,
        prompt_tokens: None,
        completion_tokens: None,
    };

    let mut messages = Vec::new();

    for step in steps.iter().filter(|step| !step.calls.is_empty()) {
        let tool_calls = step
            .calls
            .iter()
            .map(|call| {
                json!({
                    "id": call.id,
                    "type": "function",
                    "function": {
                        "name": call.name,
                        "arguments": serde_json::to_string(&call.arguments).unwrap_or_default(),
                    }
                })
            })
            .collect();

        messages.push(ChatMessage {
            tool_calls: Value::Array(tool_calls),
            ..message(MessageRole::Assistant, step.text.clone())
        });

        for call in &step.calls {
            let result = call
                .result
                .clone()
                .unwrap_or_else(|| UNFINISHED_RESULT.to_string());

            messages.push(ChatMessage {
                tool_call_id: Some(call.id.clone()),
                ..message(MessageRole::Tool, result)
            });
        }
    }

    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_results_keep_their_start_and_say_they_were_cut() {
        let (short, cut) = inline_result("ok");
        assert_eq!(short, "ok");
        assert!(!cut);

        let long = "日志".repeat(MAX_INLINE_RESULT_BYTES);
        let (kept, cut) = inline_result(&long);
        assert!(cut);
        assert!(kept.len() < long.len());
        assert!(kept.contains("[truncated"));
    }

    #[test]
    fn calls_are_grouped_by_the_model_call_that_asked_for_them() {
        let mut builder = TranscriptBuilder::default();

        builder.text("Checking the logs and the traces.");
        builder.start("a", "query_logs", json!({"q": "error"}));
        builder.start("b", "query_traces", json!({}));
        builder.end("a", "[]", false);
        builder.end("b", "boom", true);

        // 下一次调用只调工具、不写字，也是新的一步。
        builder.start("c", "get_service", json!({"name": "api"}));
        builder.end("c", "{}", false);
        builder.text("The API is failing.");

        assert_eq!(builder.calls(), 3);
        assert_eq!(builder.pending_text(), "The API is failing.");

        let steps = builder.into_steps();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].text, "Checking the logs and the traces.");
        assert_eq!(steps[0].calls.len(), 2);
        assert!(steps[0].calls[1].is_error);
        assert_eq!(steps[1].text, "");
        assert_eq!(steps[1].calls[0].name, "get_service");
    }

    #[test]
    fn steps_become_tool_calls_followed_by_their_results() {
        let mut builder = TranscriptBuilder::default();

        builder.start("a", "query_logs", json!({"q": "error"}));
        builder.end("a", "[1]", false);
        builder.start("b", "query_traces", json!({}));

        let messages = transcript_messages(
            &Id::from_string("chat-1"),
            &builder.into_steps(),
            TimestampMicros(1),
        );

        let roles = messages.iter().map(|m| m.role).collect::<Vec<_>>();
        assert_eq!(
            roles,
            [
                MessageRole::Assistant,
                MessageRole::Tool,
                MessageRole::Assistant,
                MessageRole::Tool
            ]
        );
        assert_eq!(messages[0].tool_calls[0]["function"]["name"], "query_logs");
        assert_eq!(
            messages[0].tool_calls[0]["function"]["arguments"],
            "{\"q\":\"error\"}"
        );
        assert_eq!(messages[1].tool_call_id.as_deref(), Some("a"));
        assert_eq!(messages[1].content, "[1]");
        assert_eq!(messages[3].content, UNFINISHED_RESULT);
    }
}
