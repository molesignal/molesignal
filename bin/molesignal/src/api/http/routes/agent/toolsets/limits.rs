// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一轮对话的运行上限，来自生效的 Agent Profile；没有 Profile 时用与 Profile 默认值相同的数，
//! 上下文用实例的设置。

use std::time::Duration;

use crate::agent::{
    chat::{DEFAULT_TEMPERATURE, LoopLimits},
    model::AgentProfile,
};

/// 没有生效的 Profile 时，一轮对话最多执行的工具调用数（与 Profile 的默认值相同）。
pub const CHAT_MAX_TOOL_CALLS: usize = 32;

/// 没有生效的 Profile 时，一轮对话的总时限（与 Profile 的默认值相同）。
pub const CHAT_MAX_DURATION: Duration = Duration::from_secs(1_800);

pub fn chat_limits(profile: Option<&AgentProfile>) -> LoopLimits {
    let Some(profile) = profile else {
        return LoopLimits {
            max_tool_calls: CHAT_MAX_TOOL_CALLS,
            max_duration: Some(CHAT_MAX_DURATION),
            temperature: Some(DEFAULT_TEMPERATURE),
            context_tokens: None,
        };
    };

    LoopLimits {
        max_tool_calls: usize::try_from(profile.max_tool_calls)
            .ok()
            .filter(|calls| *calls > 0)
            .unwrap_or(CHAT_MAX_TOOL_CALLS),
        max_duration: Some(
            u64::try_from(profile.max_investigation_secs)
                .ok()
                .filter(|secs| *secs > 0)
                .map(Duration::from_secs)
                .unwrap_or(CHAT_MAX_DURATION),
        ),
        temperature: Some(profile.temperature.unwrap_or(DEFAULT_TEMPERATURE)),
        context_tokens: u64::try_from(profile.max_context_tokens)
            .ok()
            .filter(|tokens| *tokens > 0),
    }
}
