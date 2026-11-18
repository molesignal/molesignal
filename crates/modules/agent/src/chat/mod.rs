// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent chat runtime.
//!
//! Provides:
//! - **Chat / Message** models + repo trait
//! - **ProviderAdapter** trait: abstraction over three providers, OpenAI / Anthropic /
//!   OpenAI-compatible; implementations and construction entry points are in [`providers`].
//! - **AgentLoop**: user message -> LLM -> compile-time tool registry call ->
//!   results fed back to LLM -> final assistant answer; see [`agent_loop`].
//! - **ModelCall**: a record passed to the caller for each model call; see [`model_call`].
//!
//! The actual SSE streaming handler is implemented behind cfg gates in the OSS api crate;
//! this crate provides the protocol layer.

use std::pin::Pin;

use async_trait::async_trait;
use futures::stream::Stream;
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};

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
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: Id,
    pub chat_id: Id,
    pub role: MessageRole,
    pub content: String,
    /// tool_call_id (populated when role=Tool).
    pub tool_call_id: Option<String>,
    /// tool_calls (populated when role=Assistant).
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

/// An LLM completion request (provider-neutral).
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
#[serde(rename_all = "camelCase")]
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    /// If Some, the LLM can return tool_call; otherwise, plain text.
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
#[serde(rename_all = "camelCase")]
pub struct CompletionResponse {
    /// Text content (populated when there are no tool_calls).
    pub content: Option<String>,
    /// The list of tools the LLM decides to call (same format as OpenAI tool_calls).
    pub tool_calls: Vec<RequestedToolCall>,
    /// All input, including cache hits.
    pub prompt_tokens: i32,
    /// Input tokens that hit the cache; 0 when the model service does not report them.
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

/// Provider adapter injected into AgentLoop.
/// OSS implementations can use reqwest to call the OpenAI / Anthropic API.
#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    fn provider(&self) -> Provider;
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse>;
    /// Streaming variant (change `agent-provider-adapter`): returns a stream of chunks parsed
    /// from SSE frames; the caller can forward them to the client one by one.
    /// The default implementation returns a single frame using `complete()`; actual providers
    /// should override it.
    async fn complete_stream(&self, req: CompletionRequest) -> Result<ChatStream> {
        // Default implementation: falls back to non-streaming complete(), returning a single chunk.
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

/// A single stream increment: a text fragment, an assembled tool_call, or a final usage chunk.
#[derive(Debug, Clone)]
pub enum ChunkOrToolCall {
    /// A text increment returned by the provider; AgentLoop forwards it to the client over SSE.
    Text(String),
    /// A fully assembled tool call from the provider (the adapter has joined all SSE fragments).
    ToolCall {
        id: String,
        name: String,
        /// The serialized arguments string; the caller parses JSON (consistent for partial /
        /// final).
        arguments: String,
    },
    /// A usage chunk (OpenAI appends it at the end of the stream; Anthropic includes it in
    /// `message_delta`). Input includes cache hits, with `cached_prompt_tokens` counting those
    /// hits.
    Usage {
        prompt_tokens: i32,
        cached_prompt_tokens: i32,
        completion_tokens: i32,
    },
    /// The stream ends.
    /// `finish_reason` comes from the provider ("stop" / "tool_calls" / "length").
    Done(String),
}

/// A type alias for the provider stream; trait-object-safe.
pub type ChatStream = Pin<Box<dyn Stream<Item = Result<ChunkOrToolCall>> + Send>>;

/// Events during one answer (output from streaming AgentLoop execution).
///
/// - [`Delta`] is a small piece of text the model is writing; when the call ends with tool calls,
///   these pieces are intermediate explanations.
/// - [`Chunk`] is the full final answer, immediately followed by [`Done`].
/// - [`ToolStart`] / [`ToolEnd`] are the start and result of a tool call.
/// - No events follow [`Done`] or [`Error`].
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

/// Where streaming execution sends events and how it detects that it has been stopped.
#[async_trait]
pub trait AgentSink: Send + Sync {
    async fn emit(&self, event: AgentStreamEvent);
    /// Whether execution has stopped: no more model or tool calls.
    fn is_cancelled(&self) -> bool;
    /// Returns when stopped, to interrupt an ongoing model call or tool.
    async fn cancelled(&self);
}

#[cfg(test)]
mod tests;
