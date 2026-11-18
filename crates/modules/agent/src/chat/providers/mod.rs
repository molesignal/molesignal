// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent Model Gateway provider adapter implementations.
//!
//! Three ProviderAdapter implementations:
//!
//! - [`openai::OpenAiAdapter`]: POST `{base_url}/chat/completions`, parses SSE
//!   `data: <json>` frames, extracts `choices[0].delta.content` + incremental tool_calls.
//! - [`anthropic::AnthropicAdapter`]: POST `{base_url}/v1/messages`, SSE
//!   `event: content_block_delta` / `message_delta` / `message_stop`.
//! - [`openai_compatible::OpenAiCompatibleAdapter`]: reuses OpenAi parsing with a different
//!   `base_url` (Together / vLLM / Ollama / Groq).
//!
//! All three share [`transport`]: connection timeout, read idle timeout, and limited retries.

use std::sync::Arc;

pub mod anthropic;
pub mod openai;
pub mod openai_compatible;
pub mod sse;
pub mod transport;

pub use anthropic::AnthropicAdapter;
use kernel::{Error, Result};
pub use openai::OpenAiAdapter;
pub use openai_compatible::OpenAiCompatibleAdapter;
pub use transport::TransportOptions;

use super::{Provider, ProviderAdapter};

/// Constructs the corresponding [`ProviderAdapter`] from environment variables based on
/// [`Provider`] (task 3.3 / design D5).
///
/// Convention: each provider uses two env vars, `MS_AGENT_<UPPER>_API_KEY` and `_BASE_URL`,
/// for injection; missing API key -> `Err(invalid)`.
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

/// Constructs a [`ProviderAdapter`] from a PG provider row (decrypted API key + optional base_url +
/// timeout settings).
///
/// When `base_url` is empty, uses the provider's default; OpenAI-compatible requires an explicit
/// `base_url`. An empty key returns `Err(invalid)` (disabled providers and providers without a
/// configured key should not reach here).
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
