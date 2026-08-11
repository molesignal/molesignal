// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 模型遥测。
//!
//! 原 infra LLM telemetry 模块全量迁入并改名为 agent：
//! - 派生 stream `agent_model_traces`（原 `llm_traces`）
//! - HTTP 路径 `/api/v1/agent/{stats,top_models,top_users}`（原 `/api/v1/llm/...`）
//! - feature gate：`license.has_feature("agent")` → 没有许可证返 403

pub mod fanout;
pub mod redact;
pub mod stats;

pub use fanout::{AgentFanoutHook, extract_batch};
pub use redact::redact_pii;
pub use stats::AgentStatsQuery;

/// 派生 stream 名（全局常量，wire / handler / 测试统一引用）。
pub const AGENT_STREAM: &str = "_agent_model_traces";

/// License feature 名（用于 `has_feature` 校验）。
pub const AGENT_FEATURE: &str = "agent";
