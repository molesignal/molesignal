// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent chat 运行时。
//!
//! 提供：
//! - **Chat / Message** 模型 + repo trait
//! - **ProviderAdapter** trait —— OpenAI / Anthropic / OpenAI-compatible 三 provider
//!   抽象，实装与构造入口在 [`providers`]
//! - **AgentLoop** —— 用户 message → LLM → 编译期工具注册表调用 →
//!   结果回灌 LLM → 最终 assistant 回答，见 [`agent_loop`]
//! - **ModelCall** —— 每次模型调用交给调用方的记录，见 [`model_call`]
//!
//! 实际 SSE streaming handler 由 OSS api crate cfg-gated 实装；本 crate 提供
//! 协议层。

use std::pin::Pin;

use async_trait::async_trait;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

pub mod agent_loop;
pub mod model_call;
pub mod providers;

pub use agent_loop::{
    AgentLoop, AgentLoopResult, DEFAULT_MAX_TOOL_CALLS, DEFAULT_TEMPERATURE, LoopLimits,
    RunConclusion,
};
pub use model_call::{ModelCall, ModelCallObserver, observed_complete};
pub use providers::{TransportOptions, adapter_from_env, adapter_from_parts};

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
    /// 全部输入，含命中缓存的部分。
    pub prompt_tokens: i32,
    /// 输入里命中缓存的 token；模型服务没有报时为 0。
    #[serde(default)]
    pub cached_prompt_tokens: i32,
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
    /// Streaming variant（change `agent-provider-adapter`）：返一个 SSE 帧拆解后的
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
            cached_prompt_tokens: resp.cached_prompt_tokens,
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
    /// usage 块（OpenAI 在 stream 结束附加；Anthropic 在 `message_delta` 内）。输入含命中缓存的
    /// 部分，`cached_prompt_tokens` 是其中命中的。
    Usage {
        prompt_tokens: i32,
        cached_prompt_tokens: i32,
        completion_tokens: i32,
    },
    /// 流结束。`finish_reason` 来自 provider（"stop" / "tool_calls" / "length"）。
    Done(String),
}

/// Provider stream 类型别名；trait-object-safe。
pub type ChatStream = Pin<Box<dyn Stream<Item = Result<ChunkOrToolCall>> + Send>>;

/// 一轮回答过程中发生的事（AgentLoop 流式运行的产出）。
///
/// - [`Delta`] 是模型正在写出的一小段文字；这次调用以工具调用结束时，它们是中间的说明。
/// - [`Chunk`] 是最终回答的全文，紧接着 [`Done`]。
/// - [`ToolStart`] / [`ToolEnd`] 是一次工具调用的开始与结果。
/// - [`Done`] 或 [`Error`] 之后不再有事件。
#[derive(Debug, Clone)]
pub enum AgentStreamEvent {
    Delta(String),
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

/// 流式运行把事件交到哪里，以及怎么知道被停下了。
#[async_trait]
pub trait AgentSink: Send + Sync {
    async fn emit(&self, event: AgentStreamEvent);
    /// 已经被停下：不再调用模型和工具。
    fn is_cancelled(&self) -> bool;
    /// 被停下时返回，用来打断正在进行的模型调用或工具。
    async fn cancelled(&self);
}

#[cfg(test)]
mod tests;
