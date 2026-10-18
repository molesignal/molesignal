// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 记账器：接住 Agent 循环与单次调用交出的每条 [`ModelCall`]。

use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;

use super::{AgentUsage, UsageScope, traces::trace_event};
use crate::agent::{
    chat::{ModelCall, ModelCallObserver},
    usage::{Cost, TokenCounts, UsageDelta},
};

pub struct UsageRecorder {
    usage: Arc<AgentUsage>,
    scope: UsageScope,
    totals: Mutex<UsageDelta>,
}

impl UsageRecorder {
    pub(super) fn new(usage: Arc<AgentUsage>, scope: UsageScope) -> Self {
        Self {
            usage,
            scope,
            totals: Mutex::new(UsageDelta::default()),
        }
    }

    pub fn scope(&self) -> &UsageScope {
        &self.scope
    }

    /// 到现在为止记下的调用次数、token 与成本。
    pub fn totals(&self) -> UsageDelta {
        *self.totals.lock()
    }

    /// 合计成本；没有单价时为空，不能当成 0 元。
    pub fn cost(&self) -> Option<Cost> {
        self.scope.prices.map(|prices| Cost {
            amount: self.totals().cost,
            currency: prices.currency,
        })
    }
}

#[async_trait]
impl ModelCallObserver for UsageRecorder {
    async fn observe(&self, call: ModelCall) {
        let tokens = TokenCounts {
            prompt: call.prompt_tokens,
            cached_prompt: call.cached_prompt_tokens,
            completion: call.completion_tokens,
        };
        let cost = self.scope.prices.map(|prices| Cost {
            amount: prices.cost(tokens),
            currency: prices.currency,
        });

        let delta = UsageDelta {
            calls: 1,
            unpriced_calls: i64::from(cost.is_none()),
            prompt_tokens: call.prompt_tokens.max(0),
            completion_tokens: call.completion_tokens.max(0),
            cost: cost.map_or(0.0, |cost| cost.amount),
            currency: cost.map(|cost| cost.currency),
        };

        {
            let mut totals = self.totals.lock();
            totals.calls += delta.calls;
            totals.unpriced_calls += delta.unpriced_calls;
            totals.prompt_tokens += delta.prompt_tokens;
            totals.completion_tokens += delta.completion_tokens;
            totals.cost += delta.cost;
            totals.currency = totals.currency.or(delta.currency);
        }

        self.usage.add(&self.scope, &delta).await;
        self.usage
            .enqueue_trace(trace_event(&self.scope, &call, cost));
    }
}
