// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Steps the model takes in one answer, each containing its explanation, tool calls, and tool
//! results.
//!
//! Store these with the answer (`agent_messages.tool_transcript`); for the next turn, restore them
//! as assistant messages with tool calls and tool results so the model can see the data retrieved
//! in the previous turn when answering follow-up questions. For results exceeding
//! [`MAX_INLINE_RESULT_BYTES`], keep only the beginning; the full results are in the evidence.

use kernel::{ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::chat::{ChatMessage, MessageRole};

/// Keep at most this many bytes of a tool result in the history.
pub const MAX_INLINE_RESULT_BYTES: usize = 16 * 1024;

/// The result stored in the history for a call stopped before its result arrives.
const UNFINISHED_RESULT: &str = "[the run was stopped before this call returned]";

/// A single tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    /// The result text, truncated if too long; empty if no result is available yet.
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub is_error: bool,
}

/// One model call: its initial explanation (possibly empty) and the tools it wants to call.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TranscriptStep {
    #[serde(default)]
    pub text: String,
    pub calls: Vec<TranscriptCall>,
}

/// The result as stored in the history: keep the beginning if too long and indicate truncation.
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

/// Record the steps of one answer from streaming events.
#[derive(Debug, Default)]
pub struct TranscriptBuilder {
    steps: Vec<TranscriptStep>,
    /// Text written so far in this model call.
    text: String,
}

impl TranscriptBuilder {
    pub fn text(&mut self, delta: &str) {
        self.text.push_str(delta);
    }

    /// Start a tool call.
    /// All tool calls from the same model call arrive before any results: if the previous step
    /// already has results, or more text is written before this call, this is a new step.
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

    /// Text from the last model call: the answer, or partially written text when stopped.
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

/// Restore messages for the model: one assistant message with tool calls per step, followed by
/// each tool result.
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

        // The next call starts a new step even if it only calls tools and writes no text.
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
