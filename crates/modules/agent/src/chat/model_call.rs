// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 每一次模型调用的记录。
//!
//! Agent 循环与单次调用（RCA、`_llm_eval`）在每次调用模型服务之后交出一条 [`ModelCall`]，
//! 由调用方注入的 [`ModelCallObserver`] 决定记到哪里：用量流、每日用量、预算。这里只描述
//! "这次调用发生了什么"，谁发起、属于哪个组织由观察者自己知道。

use std::{sync::Arc, time::Instant};

use async_trait::async_trait;

use super::{CompletionRequest, CompletionResponse, Provider, ProviderAdapter};
use crate::shared::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct ModelCall {
    pub provider: Provider,
    pub model: String,
    /// 全部输入，含命中缓存的部分。
    pub prompt_tokens: i64,
    /// 输入里命中缓存的部分。
    pub cached_prompt_tokens: i64,
    pub completion_tokens: i64,
    /// 从发出请求到拿完回答（流式时到流结束）。
    pub latency_ms: u64,
    pub streaming: bool,
    /// 这次回答里模型请求的工具调用数。
    pub tool_calls: usize,
    pub finish_reason: Option<String>,
    /// 调用失败时的原因；成功时为空。
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
    /// 记一次调用。不返回错误：记不上不能影响回答本身。
    async fn observe(&self, call: ModelCall);
}

/// 从 `started` 到现在的毫秒数。
pub fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// 调一次非流式补全，并把结果交给观察者。
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
