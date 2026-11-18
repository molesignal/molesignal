// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A record of each model call.
//!
//! The Agent loop and individual calls (RCA, `_llm_eval`) produce a [`ModelCall`] after each call
//! to the model service. The caller-injected [`ModelCallObserver`] decides where to record it:
//! the usage stream, daily usage, or budget. This only describes "what happened in this call";
//! the observer knows who initiated it and which organization it belongs to.

use std::{sync::Arc, time::Instant};

use async_trait::async_trait;
use kernel::Result;

use super::{CompletionRequest, CompletionResponse, Provider, ProviderAdapter};

#[derive(Debug, Clone, PartialEq)]
pub struct ModelCall {
    pub provider: Provider,
    pub model: String,
    /// All input, including the portion served from cache.
    pub prompt_tokens: i64,
    /// The portion of input served from cache.
    pub cached_prompt_tokens: i64,
    pub completion_tokens: i64,
    /// The time from sending the request to receiving the full response (stream end for streaming).
    pub latency_ms: u64,
    pub streaming: bool,
    /// The number of tool calls the model requests in this response.
    pub tool_calls: usize,
    pub finish_reason: Option<String>,
    /// The reason for a failed call; empty on success.
    pub error: Option<String>,
}

impl ModelCall {
    pub fn total_tokens(&self) -> i64 {
        self.prompt_tokens + self.completion_tokens
    }

    pub fn succeeded(&self) -> bool {
        self.error.is_none()
    }
}

#[async_trait]
pub trait ModelCallObserver: Send + Sync {
    /// Records one call.
    /// Returns no errors: failure to record must not affect the response itself.
    async fn observe(&self, call: ModelCall);
}

/// The number of milliseconds from `started` to now.
pub fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Makes one non-streaming completion call and passes the result to the observer.
pub async fn observed_complete(
    adapter: &dyn ProviderAdapter,
    request: CompletionRequest,
    observer: Option<&Arc<dyn ModelCallObserver>>,
) -> Result<CompletionResponse> {
    let model = request.model.clone();
    let started = Instant::now();
    let outcome = adapter.complete(request).await;

    if let Some(observer) = observer {
        let call = match &outcome {
            Ok(response) => ModelCall {
                provider: adapter.provider(),
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
                provider: adapter.provider(),
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

        observer.observe(call).await;
    }

    outcome
}
