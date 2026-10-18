// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一轮回答能用掉多少：工具调用次数、时间、上下文与采样温度。

use std::time::{Duration, Instant};

use crate::agent::tools::{ToolContent, ToolResult};

/// 调用方没有给上限时，一轮回答最多执行的工具调用数。
pub const DEFAULT_MAX_TOOL_CALLS: usize = 8;

/// 调用方没有给温度时用这个值：排查要稳定、可复现，不要发散。
pub const DEFAULT_TEMPERATURE: f32 = 0.2;

const TOOL_BUDGET_FINAL_SYSTEM_PROMPT: &str = "\
The tool-call budget for this answer has been exhausted. Do not call any more tools. \
Use only the tool results already present in the chat to produce the best possible final answer. \
If the evidence is insufficient, say that clearly and suggest how to narrow the request. \
Do not emit DSML, XML, function calls, tool calls, queries, or any other tool transport. \
Return only the product-facing answer in the exact format required by the response instructions.";

const TIME_BUDGET_FINAL_SYSTEM_PROMPT: &str = "\
The time budget for this answer has been used up. Do not call any more tools. \
Use only the tool results already present in the chat to produce the best possible final answer. \
If the evidence is insufficient, say that clearly and suggest how to narrow the request. \
Do not emit DSML, XML, function calls, tool calls, queries, or any other tool transport. \
Return only the product-facing answer in the exact format required by the response instructions.";

pub(crate) const TOOL_BUDGET_FALLBACK_MESSAGE: &str = "\
我已停止继续调用工具，因为本次对话的工具调用次数已达到上限。请缩小时间范围、指定数据流，或补充更具体的条件后重试。";

pub(crate) const INVALID_FINAL_RESPONSE_FALLBACK_MESSAGE: &str = "\
本次调查未能生成可展示的结论。请重试，或缩小时间范围并指定需要分析的服务和数据流。";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoopLimits {
    /// 一轮回答里最多执行几次工具调用；0 按 1 处理。
    pub max_tool_calls: usize,
    /// 一轮回答的总时限；到点后不再调用工具，让模型用已有结果收尾。
    pub max_duration: Option<Duration>,
    /// 采样温度；`None` 时不传，用模型服务的默认值。
    pub temperature: Option<f32>,
    /// 交给模型的上下文估算超过它的 80% 时先压缩历史；`None` 时用实例的设置。
    pub context_tokens: Option<u64>,
}

impl Default for LoopLimits {
    fn default() -> Self {
        Self {
            max_tool_calls: DEFAULT_MAX_TOOL_CALLS,
            max_duration: None,
            temperature: Some(DEFAULT_TEMPERATURE),
            context_tokens: None,
        }
    }
}

/// 为什么不再调用工具。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Exhaustion {
    ToolCalls,
    Time,
}

impl Exhaustion {
    pub(crate) const fn finish_reason(self) -> &'static str {
        match self {
            Self::ToolCalls => "tool_budget_exhausted",
            Self::Time => "time_budget_exhausted",
        }
    }

    /// 收尾那次调用前追加给模型的指令。
    pub(crate) const fn final_prompt(self) -> &'static str {
        match self {
            Self::ToolCalls => TOOL_BUDGET_FINAL_SYSTEM_PROMPT,
            Self::Time => TIME_BUDGET_FINAL_SYSTEM_PROMPT,
        }
    }

    /// 收尾的回答不能用时给用户看的话。
    pub(crate) const fn fallback(self) -> &'static str {
        match self {
            Self::ToolCalls => TOOL_BUDGET_FALLBACK_MESSAGE,
            Self::Time => INVALID_FINAL_RESPONSE_FALLBACK_MESSAGE,
        }
    }

    /// 额度用完后没有执行的工具调用回给模型的结果，保证每个调用都有对应的结果。
    pub(crate) fn skipped_result(self) -> ToolResult {
        let text = match self {
            Self::ToolCalls => "Not executed: the tool-call budget for this answer is used up.",
            Self::Time => "Not executed: the time budget for this answer is used up.",
        };

        ToolResult {
            content: vec![ToolContent::Text { text: text.into() }],
            is_error: true,
        }
    }
}

/// 一轮回答剩下的额度。
#[derive(Debug)]
pub(crate) struct Budget {
    remaining: usize,
    deadline: Option<Instant>,
}

impl Budget {
    pub(crate) fn start(limits: &LoopLimits) -> Self {
        Self {
            remaining: limits.max_tool_calls.max(1),
            deadline: limits
                .max_duration
                .map(|duration| Instant::now() + duration),
        }
    }

    /// 现在是否已经不能再调用工具。
    pub(crate) fn exhausted(&self) -> Option<Exhaustion> {
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Some(Exhaustion::Time);
        }

        (self.remaining == 0).then_some(Exhaustion::ToolCalls)
    }

    /// 占用一次工具调用；额度不够时返回原因，不占用。
    pub(crate) fn take(&mut self) -> Result<(), Exhaustion> {
        if let Some(reason) = self.exhausted() {
            return Err(reason);
        }

        self.remaining -= 1;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_calls_are_counted_down_to_zero() {
        let mut budget = Budget::start(&LoopLimits {
            max_tool_calls: 2,
            ..LoopLimits::default()
        });

        assert_eq!(budget.take(), Ok(()));
        assert_eq!(budget.take(), Ok(()));
        assert_eq!(budget.take(), Err(Exhaustion::ToolCalls));
        assert_eq!(budget.exhausted(), Some(Exhaustion::ToolCalls));
    }

    #[test]
    fn zero_tool_calls_still_allows_one() {
        let mut budget = Budget::start(&LoopLimits {
            max_tool_calls: 0,
            ..LoopLimits::default()
        });

        assert_eq!(budget.take(), Ok(()));
        assert_eq!(budget.take(), Err(Exhaustion::ToolCalls));
    }

    #[test]
    fn a_passed_deadline_stops_tool_calls_before_the_count_does() {
        let mut budget = Budget::start(&LoopLimits {
            max_tool_calls: 10,
            max_duration: Some(Duration::ZERO),
            ..LoopLimits::default()
        });

        assert_eq!(budget.exhausted(), Some(Exhaustion::Time));
        assert_eq!(budget.take(), Err(Exhaustion::Time));
    }
}
