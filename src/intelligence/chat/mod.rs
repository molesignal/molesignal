// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Intelligence 的 Mole Agent chat 运行时。
//!
//! 提供：
//! - **Chat / Message** 模型 + repo trait
//! - **ProviderAdapter** trait —— OpenAI / Anthropic / OpenAI-compatible 三 provider
//!   抽象；具体 HTTP 客户端由 OSS 注入（避免传染 reqwest dep 到此 crate）
//! - **AgentLoop** —— 用户 message → LLM → 编译期工具注册表调用 →
//!   结果回灌 LLM → 最终 assistant 回答；全程写 `intelligence_model_traces`
//!
//! 实际 SSE streaming handler 由 OSS api crate cfg-gated 实装；本 crate 提供
//! 协议层。

use std::{pin::Pin, sync::Arc};

use async_trait::async_trait;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};

use crate::{
    intelligence::tools::{ToolAuthContext, ToolCall, ToolDispatcher, ToolResult},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub mod providers;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    OpenAi,
    Anthropic,
    OpenAiCompatible,
}

impl Provider {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
            Self::OpenAiCompatible => "openai_compatible",
        }
    }
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "openai" => Ok(Self::OpenAi),
            "anthropic" => Ok(Self::Anthropic),
            "openai_compatible" => Ok(Self::OpenAiCompatible),
            other => Err(Error::invalid(format!("unknown provider: {other}"))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chat {
    pub id: Id,
    pub org_id: Id,
    pub user_id: Id,
    pub provider: Provider,
    pub model: String,
    pub created_at: TimestampMicros,
    pub last_message_at: TimestampMicros,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    Tool,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: Id,
    pub chat_id: Id,
    pub role: MessageRole,
    pub content: String,
    /// tool_call_id（role=Tool 时填）
    pub tool_call_id: Option<String>,
    /// tool_calls（role=Assistant 时填）
    pub tool_calls: serde_json::Value,
    pub created_at: TimestampMicros,
    pub prompt_tokens: Option<i32>,
    pub completion_tokens: Option<i32>,
}

#[async_trait]
pub trait ChatRepository: Send + Sync {
    async fn create_chat(&self, s: Chat) -> Result<Chat>;
    async fn get_chat(&self, org_id: &Id, id: &Id) -> Result<Chat>;
    async fn list_chats(&self, org_id: &Id, user_id: &Id, limit: i64) -> Result<Vec<Chat>>;
    async fn touch_chat(&self, id: &Id, ts: TimestampMicros) -> Result<()>;

    async fn append_message(&self, m: ChatMessage) -> Result<ChatMessage>;
    async fn list_messages(&self, chat_id: &Id) -> Result<Vec<ChatMessage>>;
}

/// LLM completion 请求（provider 中立）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoice {
    #[default]
    Auto,
    None,
    Required,
    Specific(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    /// 若为 Some，LLM 可以返 tool_call；否则纯文本
    pub tools: Option<serde_json::Value>,
    #[serde(default)]
    pub tool_choice: ToolChoice,
    pub max_tokens: Option<i32>,
    pub temperature: Option<f32>,
}

impl CompletionRequest {
    pub fn validate_tool_choice(&self) -> Result<()> {
        match &self.tool_choice {
            ToolChoice::Auto | ToolChoice::None => Ok(()),
            ToolChoice::Required if self.tools.is_some() => Ok(()),
            ToolChoice::Required => Err(Error::invalid(
                "required tool choice needs an advertised tool schema",
            )),
            ToolChoice::Specific(name) => {
                let exists = self
                    .tools
                    .as_ref()
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                    .any(|tool| {
                        tool.get("name")
                            .or_else(|| tool.pointer("/function/name"))
                            .and_then(serde_json::Value::as_str)
                            == Some(name.as_str())
                    });
                if exists {
                    Ok(())
                } else {
                    Err(Error::invalid(format!(
                        "specific tool `{name}` is not present in the filtered advertised schema"
                    )))
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionResponse {
    /// 文本内容（不含 tool_calls 时填）
    pub content: Option<String>,
    /// LLM 决定要调用的 tool 列表（OpenAI tool_calls 同形态）
    pub tool_calls: Vec<RequestedToolCall>,
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub finish_reason: String, // "stop" | "tool_calls" | "length"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestedToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Provider adapter：注入到 AgentLoop。
/// OSS 实现可基于 reqwest 调 OpenAI / Anthropic API。
#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    fn provider(&self) -> Provider;
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse>;
    /// Streaming variant（change `intelligence-provider-adapter`）：返一个 SSE 帧拆解后的
    /// chunk 流；caller 可逐块转发到客户端。默认实装基于 `complete()` 单帧返回，
    /// 真实 provider 应 override。
    async fn complete_stream(&self, req: CompletionRequest) -> Result<ChatStream> {
        // 默认实装：fall back 到非流式 complete()，作为单 chunk 返回。
        let resp = self.complete(req).await?;
        let mut chunks: Vec<Result<ChunkOrToolCall>> = Vec::new();
        if let Some(text) = resp.content
            && !text.is_empty()
        {
            chunks.push(Ok(ChunkOrToolCall::Text(text)));
        }
        for tc in resp.tool_calls {
            chunks.push(Ok(ChunkOrToolCall::ToolCall {
                id: tc.id,
                name: tc.name,
                arguments: serde_json::to_string(&tc.arguments).unwrap_or_default(),
            }));
        }
        chunks.push(Ok(ChunkOrToolCall::Usage {
            prompt_tokens: resp.prompt_tokens,
            completion_tokens: resp.completion_tokens,
        }));
        chunks.push(Ok(ChunkOrToolCall::Done(resp.finish_reason)));
        Ok(Box::pin(futures::stream::iter(chunks)))
    }
}

/// 单个流增量：要么是文本片段、要么是一个聚合好的 tool_call、要么是 usage 终结块。
#[derive(Debug, Clone)]
pub enum ChunkOrToolCall {
    /// Provider 返的文本增量；AgentLoop 把它转发到客户端 SSE。
    Text(String),
    /// Provider 聚合完成的 tool call（adapter 内部已经把所有 SSE 分片拼好）。
    ToolCall {
        id: String,
        name: String,
        /// arguments 序列化后字符串；caller 自己 parse JSON（兼容 partial / final 一致）。
        arguments: String,
    },
    /// usage 块（OpenAI 在 stream 结束附加；Anthropic 在 `message_delta` 内）。
    Usage {
        prompt_tokens: i32,
        completion_tokens: i32,
    },
    /// 流结束。`finish_reason` 来自 provider（"stop" / "tool_calls" / "length"）。
    Done(String),
}

/// Provider stream 类型别名；trait-object-safe。
pub type ChatStream = Pin<Box<dyn Stream<Item = Result<ChunkOrToolCall>> + Send>>;

/// 按 [`Provider`] 从环境变量构造对应的 [`ProviderAdapter`]
/// （task 3.3 / design D5）。
///
/// 约定：每个 provider 用 `MS_INTELLIGENCE_<UPPER>_API_KEY` 与 `_BASE_URL` 两个
/// env var 注入；缺 API key → `Err(invalid)`。
pub fn adapter_from_env(provider: Provider) -> Result<Arc<dyn ProviderAdapter>> {
    let prefix = match provider {
        Provider::OpenAi => "MS_INTELLIGENCE_OPENAI",
        Provider::Anthropic => "MS_INTELLIGENCE_ANTHROPIC",
        Provider::OpenAiCompatible => "MS_INTELLIGENCE_COMPATIBLE",
    };
    match provider {
        Provider::OpenAi => Ok(providers::openai::from_env(prefix)?),
        Provider::Anthropic => Ok(providers::anthropic::from_env(prefix)?),
        Provider::OpenAiCompatible => Ok(providers::openai_compatible::from_env(prefix)?),
    }
}

/// 从 PG provider 行（解密后的 API key + 可选 base_url）构造 [`ProviderAdapter`]
/// Mole Intelligence 模型服务适配。
///
/// `base_url` 为空时按 provider 取默认；OpenAI-compatible 必须显式提供 base_url。
/// 空 key → `Err(invalid)`（disabled / 未配置 key 的 provider 不应走到这里）。
pub fn adapter_from_parts(
    provider: Provider,
    base_url: Option<String>,
    api_key: String,
) -> Result<Arc<dyn ProviderAdapter>> {
    if api_key.is_empty() {
        return Err(Error::invalid("provider api key is empty"));
    }
    let base_url = base_url.filter(|s| !s.trim().is_empty());
    let adapter: Arc<dyn ProviderAdapter> = match provider {
        Provider::OpenAi => {
            let url = base_url.unwrap_or_else(|| "https://api.openai.com/v1".into());
            Arc::new(providers::openai::OpenAiAdapter::new(url, api_key))
        }
        Provider::Anthropic => {
            let url = base_url.unwrap_or_else(|| "https://api.anthropic.com".into());
            Arc::new(providers::anthropic::AnthropicAdapter::new(url, api_key))
        }
        Provider::OpenAiCompatible => {
            let url = base_url
                .ok_or_else(|| Error::invalid("openai_compatible provider requires base_url"))?;
            Arc::new(providers::openai_compatible::OpenAiCompatibleAdapter::new(
                url, api_key,
            ))
        }
    };
    Ok(adapter)
}

/// HTTP/SSE 客户端可见的事件流（AgentLoop::run_stream 的产出）。
///
/// HTTP handler 把它映射成 SSE：
/// - [`Chunk`] → `event: chunk\ndata: <text>\n\n`
/// - [`ToolStart`] → `event: tool_start\ndata: <json>\n\n`
/// - [`ToolEnd`] → `event: tool_end\ndata: <json>\n\n`
/// - [`Done`] → `event: done\ndata: <json>\n\n` 然后 stream 结束
/// - [`Error`] → `event: error\ndata: <json>\n\n` 然后 stream 结束
#[derive(Debug, Clone)]
pub enum AgentStreamEvent {
    Chunk(String),
    ToolStart {
        id: String,
        name: String,
        arguments: String,
    },
    ToolEnd {
        id: String,
        result: String,
        is_error: bool,
    },
    Done {
        content: String,
        prompt_tokens: i32,
        completion_tokens: i32,
        finish_reason: String,
    },
    Error(String),
}

pub type AgentEventStream = Pin<Box<dyn Stream<Item = AgentStreamEvent> + Send>>;

/// 工具调用循环上限（避免 LLM 无限调工具）。
pub const MAX_TOOL_LOOPS: usize = 8;

const TOOL_BUDGET_FINAL_SYSTEM_PROMPT: &str = "\
The tool-call budget for this answer has been exhausted. Do not call any more tools. \
Use only the tool results already present in the chat to produce the best possible final answer. \
If the evidence is insufficient, say that clearly and suggest how to narrow the request. \
Do not emit DSML, XML, function calls, tool calls, queries, or any other tool transport. \
Return only the product-facing answer in the exact format required by the response instructions.";

const TOOL_BUDGET_FALLBACK_MESSAGE: &str = "\
我已停止继续调用工具，因为本次对话的工具调用次数已达到上限。请缩小时间范围、指定数据流，或补充更具体的条件后重试。";

const INVALID_FINAL_RESPONSE_FALLBACK_MESSAGE: &str = "\
本次调查未能生成可展示的结论。请重试，或缩小时间范围并指定需要分析的服务和数据流。";

fn contains_internal_tool_transport(content: &str) -> bool {
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

fn tool_budget_final_prompt() -> ChatMessage {
    ChatMessage {
        id: Id::new(),
        chat_id: Id(String::new()),
        role: MessageRole::System,
        content: TOOL_BUDGET_FINAL_SYSTEM_PROMPT.into(),
        tool_call_id: None,
        tool_calls: serde_json::Value::Null,
        created_at: TimestampMicros::now(),
        prompt_tokens: None,
        completion_tokens: None,
    }
}

fn tool_failure_result(error: Error) -> ToolResult {
    let content = match error {
        Error::Validation { message, issues } => crate::intelligence::tools::ToolContent::Json {
            json: serde_json::json!({
                "error": "validation_failed",
                "message": message,
                "issues": issues,
            }),
        },
        other => crate::intelligence::tools::ToolContent::Text {
            text: format!("tool error: {other}"),
        },
    };
    ToolResult {
        content: vec![content],
        is_error: true,
    }
}

/// AgentLoop：执行 user → LLM → tool → LLM → ... → final assistant。
pub struct AgentLoop {
    provider: Arc<dyn ProviderAdapter>,
    dispatcher: Arc<dyn ToolDispatcher>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLoopResult {
    pub assistant_content: String,
    pub tool_calls_made: usize,
    pub prompt_tokens_total: i32,
    pub completion_tokens_total: i32,
}

impl AgentLoop {
    pub fn new(provider: Arc<dyn ProviderAdapter>, dispatcher: Arc<dyn ToolDispatcher>) -> Self {
        Self {
            provider,
            dispatcher,
        }
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

        for loop_idx in 0..MAX_TOOL_LOOPS {
            let req = CompletionRequest {
                model: model.clone(),
                messages: messages.clone(),
                tools: tools_schema.clone(),
                tool_choice: if loop_idx == 0 {
                    initial_tool_choice.clone()
                } else {
                    ToolChoice::Auto
                },
                max_tokens,
                temperature: Some(0.2),
            };
            req.validate_tool_choice()?;
            let resp = self.provider.complete(req).await?;
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
                });
            }

            // OpenAI/兼容 API 要求：tool 结果消息前必须先有一条携带 tool_calls 的 assistant
            // 消息，否则 400 "Messages with role 'tool' must be a response to a preceding
            // message with 'tool_calls'"。
            messages.push(ChatMessage {
                id: Id::new(),
                chat_id: Id(String::new()),
                role: MessageRole::Assistant,
                content: resp.content.clone().unwrap_or_default(),
                tool_call_id: None,
                tool_calls: serde_json::Value::Array(
                    resp.tool_calls
                        .iter()
                        .map(|tc| {
                            serde_json::json!({
                                "id": tc.id.clone(),
                                "type": "function",
                                "function": {
                                    "name": tc.name.clone(),
                                    "arguments": serde_json::to_string(&tc.arguments)
                                        .unwrap_or_default(),
                                }
                            })
                        })
                        .collect(),
                ),
                created_at: TimestampMicros::now(),
                prompt_tokens: None,
                completion_tokens: None,
            });
            // 跑 tool calls，把结果作为 Tool message 追加，再回 LLM
            for tc in &resp.tool_calls {
                tool_calls_made += 1;
                let call = ToolCall {
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                };
                let result: ToolResult = self
                    .dispatcher
                    .dispatch(ctx, call)
                    .await
                    .unwrap_or_else(tool_failure_result);
                let result_json = serde_json::to_string(&result.content).unwrap_or_default();
                messages.push(ChatMessage {
                    id: Id::new(),
                    chat_id: Id("".into()), // 占位；持久化时由 caller 填
                    role: MessageRole::Tool,
                    content: result_json,
                    tool_call_id: Some(tc.id.clone()),
                    tool_calls: serde_json::Value::Null,
                    created_at: TimestampMicros::now(),
                    prompt_tokens: None,
                    completion_tokens: None,
                });
            }

            tracing::debug!(
                loop_idx = loop_idx,
                tool_calls_made = tool_calls_made,
                "chat loop continuing after tool calls"
            );
        }

        messages.push(tool_budget_final_prompt());
        let req = CompletionRequest {
            model,
            messages,
            tools: None,
            tool_choice: ToolChoice::None,
            max_tokens,
            temperature: Some(0.2),
        };
        let resp = self.provider.complete(req).await?;
        total_prompt += resp.prompt_tokens;
        total_completion += resp.completion_tokens;
        let content = resp.content.unwrap_or_default();
        Ok(AgentLoopResult {
            assistant_content: product_answer_or_fallback(&content, TOOL_BUDGET_FALLBACK_MESSAGE),
            tool_calls_made,
            prompt_tokens_total: total_prompt,
            completion_tokens_total: total_completion,
        })
    }

    /// Streaming run（change `intelligence-provider-adapter` 2.x）：把 provider 增量
    /// + tool 调用结果以 [`AgentStreamEvent`] 流式输出给 caller，caller 把它
    ///   转成 SSE 帧丢给浏览器。
    ///
    /// 失败处理：provider 错 → 发一条 `AgentStreamEvent::Error` 然后 close stream。
    /// MAX_TOOL_LOOPS 超时会再执行一次无工具最终回答，避免把内部预算错误直接暴露给用户。
    pub fn run_stream(
        self: Arc<Self>,
        ctx: ToolAuthContext,
        model: String,
        history: Vec<ChatMessage>,
        tools_schema: Option<serde_json::Value>,
        max_tokens: Option<i32>,
    ) -> AgentEventStream {
        self.run_stream_with_tool_choice(
            ctx,
            model,
            history,
            tools_schema,
            max_tokens,
            ToolChoice::Auto,
        )
    }

    pub fn run_stream_with_tool_choice(
        self: Arc<Self>,
        ctx: ToolAuthContext,
        model: String,
        history: Vec<ChatMessage>,
        tools_schema: Option<serde_json::Value>,
        max_tokens: Option<i32>,
        initial_tool_choice: ToolChoice,
    ) -> AgentEventStream {
        use futures::StreamExt;
        let (tx, rx) = tokio::sync::mpsc::channel::<AgentStreamEvent>(64);
        crate::shared::trace_context::spawn_with_current_trace_context(async move {
            let mut messages = history;
            let mut total_prompt = 0i32;
            let mut total_completion = 0i32;
            let mut assistant_content = String::new();
            let mut finish_reason = String::from("stop");
            for loop_idx in 0..MAX_TOOL_LOOPS {
                let req = CompletionRequest {
                    model: model.clone(),
                    messages: messages.clone(),
                    tools: tools_schema.clone(),
                    tool_choice: if loop_idx == 0 {
                        initial_tool_choice.clone()
                    } else {
                        ToolChoice::Auto
                    },
                    max_tokens,
                    temperature: Some(0.2),
                };
                if let Err(error) = req.validate_tool_choice() {
                    let _ = tx.send(AgentStreamEvent::Error(error.to_string())).await;
                    return;
                }
                let mut stream = match self.provider.complete_stream(req).await {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = tx.send(AgentStreamEvent::Error(e.to_string())).await;
                        return;
                    }
                };
                let mut iter_text = String::new();
                let mut tool_calls: Vec<RequestedToolCall> = Vec::new();
                while let Some(item) = stream.next().await {
                    match item {
                        Ok(ChunkOrToolCall::Text(t)) => {
                            iter_text.push_str(&t);
                        }
                        Ok(ChunkOrToolCall::ToolCall {
                            id,
                            name,
                            arguments,
                        }) => {
                            let _ = tx
                                .send(AgentStreamEvent::ToolStart {
                                    id: id.clone(),
                                    name: name.clone(),
                                    arguments: arguments.clone(),
                                })
                                .await;
                            tool_calls.push(RequestedToolCall {
                                id,
                                name,
                                arguments: serde_json::from_str(&arguments)
                                    .unwrap_or(serde_json::Value::String(arguments)),
                            });
                        }
                        Ok(ChunkOrToolCall::Usage {
                            prompt_tokens,
                            completion_tokens,
                        }) => {
                            total_prompt += prompt_tokens;
                            total_completion += completion_tokens;
                        }
                        Ok(ChunkOrToolCall::Done(r)) => {
                            finish_reason = r;
                        }
                        Err(e) => {
                            let _ = tx.send(AgentStreamEvent::Error(e.to_string())).await;
                            return;
                        }
                    }
                }
                if tool_calls.is_empty() {
                    let final_output = product_answer_or_fallback(
                        &iter_text,
                        INVALID_FINAL_RESPONSE_FALLBACK_MESSAGE,
                    );
                    assistant_content.push_str(&final_output);
                    let _ = tx.send(AgentStreamEvent::Chunk(final_output)).await;
                    let _ = tx
                        .send(AgentStreamEvent::Done {
                            content: assistant_content,
                            prompt_tokens: total_prompt,
                            completion_tokens: total_completion,
                            finish_reason,
                        })
                        .await;
                    return;
                }
                // OpenAI/兼容 API 要求：tool 结果消息前必须先有一条携带 tool_calls 的 assistant
                // 消息，否则 400 "Messages with role 'tool' must be a response to a preceding
                // message with 'tool_calls'"。
                messages.push(ChatMessage {
                    id: Id::new(),
                    chat_id: Id(String::new()),
                    role: MessageRole::Assistant,
                    content: iter_text.clone(),
                    tool_call_id: None,
                    tool_calls: serde_json::Value::Array(
                        tool_calls
                            .iter()
                            .map(|tc| {
                                serde_json::json!({
                                    "id": tc.id.clone(),
                                    "type": "function",
                                    "function": {
                                        "name": tc.name.clone(),
                                        "arguments": serde_json::to_string(&tc.arguments)
                                            .unwrap_or_default(),
                                    }
                                })
                            })
                            .collect(),
                    ),
                    created_at: TimestampMicros::now(),
                    prompt_tokens: None,
                    completion_tokens: None,
                });
                // 跑 tools 一遍并把结果作为 Tool message 追加。
                for tc in &tool_calls {
                    let call = ToolCall {
                        name: tc.name.clone(),
                        arguments: tc.arguments.clone(),
                    };
                    let result: ToolResult = match self.dispatcher.dispatch(&ctx, call).await {
                        Ok(r) => r,
                        Err(e) => tool_failure_result(e),
                    };
                    let result_json = serde_json::to_string(&result.content).unwrap_or_default();
                    let _ = tx
                        .send(AgentStreamEvent::ToolEnd {
                            id: tc.id.clone(),
                            result: result_json.clone(),
                            is_error: result.is_error,
                        })
                        .await;
                    messages.push(ChatMessage {
                        id: Id::new(),
                        chat_id: Id("".into()),
                        role: MessageRole::Tool,
                        content: result_json,
                        tool_call_id: Some(tc.id.clone()),
                        tool_calls: serde_json::Value::Null,
                        created_at: TimestampMicros::now(),
                        prompt_tokens: None,
                        completion_tokens: None,
                    });
                }
                // 中间轮次的说明只回灌给模型，不进入用户可见回答。
            }
            messages.push(tool_budget_final_prompt());
            let req = CompletionRequest {
                model,
                messages,
                tools: None,
                tool_choice: ToolChoice::None,
                max_tokens,
                temperature: Some(0.2),
            };
            let mut stream = match self.provider.complete_stream(req).await {
                Ok(s) => s,
                Err(_) => {
                    assistant_content.push_str(TOOL_BUDGET_FALLBACK_MESSAGE);
                    let _ = tx
                        .send(AgentStreamEvent::Chunk(TOOL_BUDGET_FALLBACK_MESSAGE.into()))
                        .await;
                    let _ = tx
                        .send(AgentStreamEvent::Done {
                            content: assistant_content,
                            prompt_tokens: total_prompt,
                            completion_tokens: total_completion,
                            finish_reason: "tool_budget_exhausted".into(),
                        })
                        .await;
                    return;
                }
            };
            let mut final_text = String::new();
            while let Some(item) = stream.next().await {
                match item {
                    Ok(ChunkOrToolCall::Text(t)) => {
                        final_text.push_str(&t);
                    }
                    Ok(ChunkOrToolCall::Usage {
                        prompt_tokens,
                        completion_tokens,
                    }) => {
                        total_prompt += prompt_tokens;
                        total_completion += completion_tokens;
                    }
                    Ok(ChunkOrToolCall::Done(_)) => {}
                    Ok(ChunkOrToolCall::ToolCall { .. }) => {}
                    Err(_) => break,
                }
            }
            let final_output =
                product_answer_or_fallback(&final_text, TOOL_BUDGET_FALLBACK_MESSAGE);
            assistant_content.push_str(&final_output);
            let _ = tx.send(AgentStreamEvent::Chunk(final_output)).await;
            let _ = tx
                .send(AgentStreamEvent::Done {
                    content: assistant_content,
                    prompt_tokens: total_prompt,
                    completion_tokens: total_completion,
                    finish_reason: "tool_budget_exhausted".into(),
                })
                .await;
        });
        Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx))
    }
}

#[cfg(test)]
mod tests;
