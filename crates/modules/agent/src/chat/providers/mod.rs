// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent Model Gateway provider adapter 实装。
//!
//! 三个 ProviderAdapter：
//!
//! - [`openai::OpenAiAdapter`]：POST `{base_url}/chat/completions`，SSE 解析
//!   `data: <json>` 帧，提取 `choices[0].delta.content` + 增量 tool_calls。
//! - [`anthropic::AnthropicAdapter`]：POST `{base_url}/v1/messages`，SSE
//!   `event: content_block_delta` / `message_delta` / `message_stop`。
//! - [`openai_compatible::OpenAiCompatibleAdapter`]：复用 OpenAi 解析；不同
//!   `base_url`（Together / vLLM / Ollama / Groq）。
//!
//! 三者共用 [`transport`]：连接超时、读空闲超时与有限重试。

use std::sync::Arc;

pub mod anthropic;
pub mod openai;
pub mod openai_compatible;
pub mod sse;
pub mod transport;

pub use anthropic::AnthropicAdapter;
pub use openai::OpenAiAdapter;
pub use openai_compatible::OpenAiCompatibleAdapter;
pub use transport::TransportOptions;

use super::{Provider, ProviderAdapter};
use crate::shared::{Error, Result};

/// 按 [`Provider`] 从环境变量构造对应的 [`ProviderAdapter`]
/// （task 3.3 / design D5）。
///
/// 约定：每个 provider 用 `MS_AGENT_<UPPER>_API_KEY` 与 `_BASE_URL` 两个
/// env var 注入；缺 API key → `Err(invalid)`。
pub fn adapter_from_env(provider: Provider) -> Result<Arc<dyn ProviderAdapter>> {
    let prefix = match provider {
        Provider::OpenAi => "MS_AGENT_OPENAI",
        Provider::Anthropic => "MS_AGENT_ANTHROPIC",
        Provider::OpenAiCompatible => "MS_AGENT_COMPATIBLE",
    };

    match provider {
        Provider::OpenAi => Ok(openai::from_env(prefix)?),
        Provider::Anthropic => Ok(anthropic::from_env(prefix)?),
        Provider::OpenAiCompatible => Ok(openai_compatible::from_env(prefix)?),
    }
}

/// 从 PG provider 行（解密后的 API key + 可选 base_url + 超时设置）构造 [`ProviderAdapter`]。
///
/// `base_url` 为空时按 provider 取默认；OpenAI-compatible 必须显式提供 base_url。
/// 空 key → `Err(invalid)`（disabled / 未配置 key 的 provider 不应走到这里）。
pub fn adapter_from_parts(
    provider: Provider,
    base_url: Option<String>,
    api_key: String,
    transport: TransportOptions,
) -> Result<Arc<dyn ProviderAdapter>> {
    if api_key.is_empty() {
        return Err(Error::invalid("provider api key is empty"));
    }

    let base_url = base_url.filter(|s| !s.trim().is_empty());

    let adapter: Arc<dyn ProviderAdapter> = match provider {
        Provider::OpenAi => {
            let url = base_url.unwrap_or_else(|| "https://api.openai.com/v1".into());
            Arc::new(OpenAiAdapter::new(url, api_key).with_transport(transport))
        }
        Provider::Anthropic => {
            let url = base_url.unwrap_or_else(|| "https://api.anthropic.com".into());
            Arc::new(AnthropicAdapter::new(url, api_key).with_transport(transport))
        }
        Provider::OpenAiCompatible => {
            let url = base_url
                .ok_or_else(|| Error::invalid("openai_compatible provider requires base_url"))?;

            Arc::new(OpenAiCompatibleAdapter::new(url, api_key).with_transport(transport))
        }
    };

    Ok(adapter)
}
