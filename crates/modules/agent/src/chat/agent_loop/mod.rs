// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! AgentLoop: runs user -> LLM -> tool -> LLM -> ... -> final assistant.
//!
//! [`LoopLimits`] determines the budget for one answer: tool call count, total time limit, and
//! temperature. Once the budget runs out, stops calling tools and appends a final instruction
//! so the model gives its final answer using results already obtained. Each model service call
//! passes a [`ModelCall`] to the observer.

use std::{sync::Arc, time::Instant};

use budget::{Budget, Exhaustion, INVALID_FINAL_RESPONSE_FALLBACK_MESSAGE};
use futures::StreamExt;
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};
use tool_runtime::{ToolContent, ToolResult};

use super::{
    ChatMessage, CompletionRequest, CompletionResponse, MessageRole, ProviderAdapter,
    RequestedToolCall, ToolChoice,
    model_call::{ModelCall, ModelCallObserver, elapsed_ms},
};
use crate::tools::{ToolAuthContext, ToolDispatcher};

mod budget;
mod group;
mod stream;
#[cfg(test)]
mod tests;

pub use budget::{DEFAULT_MAX_TOOL_CALLS, DEFAULT_TEMPERATURE, LoopLimits};
pub use stream::RunConclusion;

pub(crate) fn contains_internal_tool_transport(content: &str) -> bool {
    let normalized = content.replace('｜', "|").to_ascii_lowercase();

    normalized.contains("dsml")
        && (normalized.contains("tool_calls")
            || normalized.contains("invoke")
            || normalized.contains("parameter"))
}

fn product_answer_or_fallback(content: &str, fallback: &str) -> String {
    if content.trim().is_empty() || contains_internal_tool_transport(content) {
        fallback.to_string()
    } else {
        content.to_string()
    }
}

fn tool_failure_result(error: Error) -> ToolResult {
    let content = match error {
        Error::Validation { message, issues } => ToolContent::Json {
            json: serde_json::json!({
                "error": "validation_failed",
                "message": message,
                "issues": issues,
            }),
        },
        other => ToolContent::Text {
            text: format!("tool error: {other}"),
        },
    };

    ToolResult {
        content: vec![content],
        is_error: true,
    }
}

fn message(role: MessageRole, content: String) -> ChatMessage {
    ChatMessage {
        id: Id::new(),
        chat_id: Id(String::new()),
        role,
        content,
        tool_call_id: None,
        tool_calls: serde_json::Value::Null,
        created_at: TimestampMicros::now(),
        prompt_tokens: None,
        completion_tokens: None,
    }
}

/// An assistant message carrying tool calls.
/// OpenAI and compatible APIs require this message before tool results; otherwise, they return
/// 400 "Messages with role 'tool' must be a response to a preceding message with
/// 'tool_calls'". The Anthropic adapter converts it to `tool_use` blocks.
fn assistant_with_tool_calls(content: String, calls: &[RequestedToolCall]) -> ChatMessage {
    let tool_calls = calls
        .iter()
        .map(|tc| {
            serde_json::json!({
                "id": tc.id.clone(),
                "type": "function",
                "function": {
                    "name": tc.name.clone(),
                    "arguments": serde_json::to_string(&tc.arguments).unwrap_or_default(),
                }
            })
        })
        .collect();

    ChatMessage {
        tool_calls: serde_json::Value::Array(tool_calls),
        ..message(MessageRole::Assistant, content)
    }
}

/// A tool result message, along with JSON text returned to the caller for display.
fn tool_message(call_id: &str, result: &ToolResult) -> (ChatMessage, String) {
    let content = serde_json::to_string(&result.content).unwrap_or_default();

    let chat_message = ChatMessage {
        tool_call_id: Some(call_id.to_string()),
        ..message(MessageRole::Tool, content.clone())
    };

    (chat_message, content)
}

/// AgentLoop: runs user -> LLM -> tool -> LLM -> ... -> final assistant.
pub struct AgentLoop {
    provider: Arc<dyn ProviderAdapter>,
    dispatcher: Arc<dyn ToolDispatcher>,
    limits: LoopLimits,
    observer: Option<Arc<dyn ModelCallObserver>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLoopResult {
    pub assistant_content: String,
    pub tool_calls_made: usize,
    pub prompt_tokens_total: i32,
    pub completion_tokens_total: i32,
    /// The finish reason of the last model call; `tool_budget_exhausted` or
    /// `time_budget_exhausted` when the budget runs out.
    pub finish_reason: String,
}

impl AgentLoop {
    pub fn new(provider: Arc<dyn ProviderAdapter>, dispatcher: Arc<dyn ToolDispatcher>) -> Self {
        Self {
            provider,
            dispatcher,
            limits: LoopLimits::default(),
            observer: None,
        }
    }

    pub fn with_limits(mut self, limits: LoopLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Passes the record to `observer` after each model service call.
    pub fn with_observer(mut self, observer: Arc<dyn ModelCallObserver>) -> Self {
        self.observer = Some(observer);
        self
    }

    pub fn limits(&self) -> &LoopLimits {
        &self.limits
    }

    fn request(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
        tools: Option<serde_json::Value>,
        tool_choice: ToolChoice,
        max_tokens: Option<i32>,
    ) -> CompletionRequest {
        CompletionRequest {
            model: model.to_string(),
            messages,
            tools,
            tool_choice,
            max_tokens,
            temperature: self.limits.temperature,
        }
    }

    async fn observe(&self, call: ModelCall) {
        if let Some(observer) = &self.observer {
            observer.observe(call).await;
        }
    }

    /// Makes one non-streaming model service call and records it.
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        let model = request.model.clone();
        let started = Instant::now();
        let outcome = self.provider.complete(request).await;

        let call = match &outcome {
            Ok(response) => ModelCall {
                provider: self.provider.provider(),
                model,
                prompt_tokens: i64::from(response.prompt_tokens),
                cached_prompt_tokens: i64::from(response.cached_prompt_tokens),
                completion_tokens: i64::from(response.completion_tokens),
                latency_ms: elapsed_ms(started),
                streaming: false,
                tool_calls: response.tool_calls.len(),
                finish_reason: Some(response.finish_reason.clone()),
                error: None,
            },
            Err(error) => ModelCall {
                provider: self.provider.provider(),
                model,
                prompt_tokens: 0,
                cached_prompt_tokens: 0,
                completion_tokens: 0,
                latency_ms: elapsed_ms(started),
                streaming: false,
                tool_calls: 0,
                finish_reason: None,
                error: Some(error.to_string()),
            },
        };

        self.observe(call).await;

        outcome
    }

    /// Runs the full loop.
    /// `history` contains system + historical messages + the current user message.
    /// Returns the final assistant content + counts.
    #[tracing::instrument(
        name = "gen_ai.agent",
        skip_all,
        fields(
            otel.kind = "internal",
            gen_ai.request.model = %model,
            gen_ai.provider.name = ?self.provider.provider()
        )
    )]
    pub async fn run(
        &self,
        ctx: &ToolAuthContext,
        model: String,
        history: Vec<ChatMessage>,
        tools_schema: Option<serde_json::Value>,
        max_tokens: Option<i32>,
    ) -> Result<AgentLoopResult> {
        self.run_with_tool_choice(
            ctx,
            model,
            history,
            tools_schema,
            max_tokens,
            ToolChoice::Auto,
        )
        .await
    }

    pub async fn run_with_tool_choice(
        &self,
        ctx: &ToolAuthContext,
        model: String,
        history: Vec<ChatMessage>,
        tools_schema: Option<serde_json::Value>,
        max_tokens: Option<i32>,
        initial_tool_choice: ToolChoice,
    ) -> Result<AgentLoopResult> {
        let mut messages = history;
        let mut total_prompt = 0i32;
        let mut total_completion = 0i32;
        let mut tool_calls_made = 0usize;
        let mut budget = Budget::start(&self.limits);
        let mut tool_choice = initial_tool_choice;

        let exhaustion = loop {
            if let Some(reason) = budget.exhausted() {
                break reason;
            }

            let req = self.request(
                &model,
                messages.clone(),
                tools_schema.clone(),
                std::mem::replace(&mut tool_choice, ToolChoice::Auto),
                max_tokens,
            );

            req.validate_tool_choice()?;
            let resp = self.complete(req).await?;
            total_prompt += resp.prompt_tokens;
            total_completion += resp.completion_tokens;

            if resp.tool_calls.is_empty() {
                // Final answer
                let content = resp.content.unwrap_or_default();

                return Ok(AgentLoopResult {
                    assistant_content: product_answer_or_fallback(
                        &content,
                        INVALID_FINAL_RESPONSE_FALLBACK_MESSAGE,
                    ),
                    tool_calls_made,
                    prompt_tokens_total: total_prompt,
                    completion_tokens_total: total_completion,
                    finish_reason: resp.finish_reason,
                });
            }

            messages.push(assistant_with_tool_calls(
                resp.content.clone().unwrap_or_default(),
                &resp.tool_calls,
            ));

            // Runs tool calls, appends results as Tool messages, then returns to the LLM.
            // Calls with insufficient budget are skipped but still receive a result; otherwise,
            // the next request contains calls without results. Consecutive calls that only read
            // run together; results keep the order of the calls.
            for range in group::groups(self.dispatcher.as_ref(), &resp.tool_calls) {
                let calls = &resp.tool_calls[range];
                let planned = group::plan(&mut budget, calls);
                tool_calls_made += group::runs(&planned);

                let results: Vec<ToolResult> = group::run(self.dispatcher.as_ref(), ctx, planned)
                    .collect()
                    .await;

                let mut results = results.into_iter();

                for tc in calls {
                    let result = results.next().unwrap_or_else(group::missing_result);
                    messages.push(tool_message(&tc.id, &result).0);
                }
            }

            tracing::debug!(
                tool_calls_made = tool_calls_made,
                "chat loop continuing after tool calls"
            );
        };

        messages.push(message(
            MessageRole::System,
            exhaustion.final_prompt().into(),
        ));

        let req = self.request(&model, messages, None, ToolChoice::None, max_tokens);
        let resp = self.complete(req).await?;
        total_prompt += resp.prompt_tokens;
        total_completion += resp.completion_tokens;
        let content = resp.content.unwrap_or_default();

        Ok(AgentLoopResult {
            assistant_content: product_answer_or_fallback(&content, exhaustion.fallback()),
            tool_calls_made,
            prompt_tokens_total: total_prompt,
            completion_tokens_total: total_completion,
            finish_reason: exhaustion.finish_reason().into(),
        })
    }
}
