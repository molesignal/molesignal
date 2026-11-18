// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Limits for one answer: tool call count, time, context, and sampling temperature.

use std::time::{Duration, Instant};

use tool_runtime::{ToolContent, ToolResult};

/// The maximum number of tool calls per answer when the caller provides no limit.
pub const DEFAULT_MAX_TOOL_CALLS: usize = 8;

/// The default temperature when the caller provides none: troubleshooting stays stable and
/// reproducible, without diverging.
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
    /// The maximum number of tool calls per answer; 0 is treated as 1.
    pub max_tool_calls: usize,
    /// The total time limit for one answer; once reached, stops calling tools and lets the model
    /// finish with existing results.
    pub max_duration: Option<Duration>,
    /// The sampling temperature; omitted when `None` to use the model service's default.
    pub temperature: Option<f32>,
    /// Compacts history when the estimated context sent to the model exceeds 80% of this value;
    /// `None` uses the instance setting.
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

/// The reason tools can no longer be called.
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

    /// The instruction appended for the model before the final call.
    pub(crate) const fn final_prompt(self) -> &'static str {
        match self {
            Self::ToolCalls => TOOL_BUDGET_FINAL_SYSTEM_PROMPT,
            Self::Time => TIME_BUDGET_FINAL_SYSTEM_PROMPT,
        }
    }

    /// The message shown to the user when the final answer is unusable.
    pub(crate) const fn fallback(self) -> &'static str {
        match self {
            Self::ToolCalls => TOOL_BUDGET_FALLBACK_MESSAGE,
            Self::Time => INVALID_FINAL_RESPONSE_FALLBACK_MESSAGE,
        }
    }

    /// The result returned to the model for a tool call skipped after the budget runs out,
    /// so that every call has a corresponding result.
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

/// The remaining budget for one answer.
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

    /// Whether tools can no longer be called now.
    pub(crate) fn exhausted(&self) -> Option<Exhaustion> {
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Some(Exhaustion::Time);
        }

        (self.remaining == 0).then_some(Exhaustion::ToolCalls)
    }

    /// Consumes one tool call; returns the reason without consuming it if the budget is
    /// insufficient.
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
