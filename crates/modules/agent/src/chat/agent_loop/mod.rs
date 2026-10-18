// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! AgentLoop：执行 user → LLM → tool → LLM → ... → final assistant。
//!
//! 一轮回答能用掉多少由 [`LoopLimits`] 决定：工具调用次数、总时限与温度。额度用完后
//! 不再调用工具，追加一条收尾指令，让模型用已经拿到的结果给出最终回答。每次调用模型
//! 服务都交一条 [`ModelCall`] 给观察者。

use std::{sync::Arc, time::Instant};

use budget::{Budget, Exhaustion, INVALID_FINAL_RESPONSE_FALLBACK_MESSAGE};
use serde::{Deserialize, Serialize};

use super::{
    ChatMessage, CompletionRequest, CompletionResponse, MessageRole, ProviderAdapter,
    RequestedToolCall, ToolChoice,
    model_call::{ModelCall, ModelCallObserver, elapsed_ms},
};
use crate::{
    agent::tools::{ToolAuthContext, ToolCall, ToolContent, ToolDispatcher, ToolResult},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod budget;
mod stream;

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

/// 携带工具调用的 assistant 消息。OpenAI 与兼容 API 要求工具结果前必须先有这样一条，
/// 否则 400 "Messages with role 'tool' must be a response to a preceding message with
/// 'tool_calls'"；Anthropic 适配器把它转成 `tool_use` 块。
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

/// 工具结果消息，连同回给调用方展示用的 JSON 文本。
fn tool_message(call_id: &str, result: &ToolResult) -> (ChatMessage, String) {
    let content = serde_json::to_string(&result.content).unwrap_or_default();

    let chat_message = ChatMessage {
        tool_call_id: Some(call_id.to_string()),
        ..message(MessageRole::Tool, content.clone())
    };

    (chat_message, content)
}

/// AgentLoop：执行 user → LLM → tool → LLM → ... → final assistant。
pub struct AgentLoop {
    provider: Arc<dyn ProviderAdapter>,
    dispatcher: Arc<dyn ToolDispatcher>,
    limits: LoopLimits,
    observer: Option<Arc<dyn ModelCallObserver>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLoopResult {
    pub assistant_content: String,
    pub tool_calls_made: usize,
    pub prompt_tokens_total: i32,
    pub completion_tokens_total: i32,
    /// 最后一次模型调用的结束原因；额度用完时是 `tool_budget_exhausted` 或
    /// `time_budget_exhausted`。
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

    /// 每次调用模型服务之后把记录交给 `observer`。
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

    /// 非流式调用一次模型服务，并记下这次调用。
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

    /// 跑完整循环。`history` 包含 system + 历史消息 + 当前 user message。
    /// 返回最终 assistant content + 计数。
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
                // 最终回答
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

            // 跑 tool calls，把结果作为 Tool message 追加，再回 LLM；额度不够的调用不执行，
            // 也要回一条结果，否则下一次请求里有调用没有结果。
            for tc in &resp.tool_calls {
                let result = match budget.take() {
                    Ok(()) => {
                        tool_calls_made += 1;

                        let call = ToolCall {
                            name: tc.name.clone(),
                            arguments: tc.arguments.clone(),
                        };

                        self.dispatcher
                            .dispatch(ctx, call)
                            .await
                            .unwrap_or_else(tool_failure_result)
                    }
                    Err(reason) => reason.skipped_result(),
                };

                messages.push(tool_message(&tc.id, &result).0);
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
