// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Intelligence Model Gateway provider adapter 实装。
//!
//! 三个 ProviderAdapter：
//!
//! - [`openai::OpenAiAdapter`]：POST `{base_url}/chat/completions`，SSE 解析
//!   `data: <json>` 帧，提取 `choices[0].delta.content` + 增量 tool_calls。
//! - [`anthropic::AnthropicAdapter`]：POST `{base_url}/v1/messages`，SSE
//!   `event: content_block_delta` / `message_delta` / `message_stop`。
//! - [`openai_compatible::OpenAiCompatibleAdapter`]：复用 OpenAi 解析；不同
//!   `base_url`（Together / vLLM / Ollama / Groq）。

pub mod anthropic;
pub mod openai;
pub mod openai_compatible;
pub mod sse;

pub use anthropic::AnthropicAdapter;
pub use openai::OpenAiAdapter;
pub use openai_compatible::OpenAiCompatibleAdapter;
