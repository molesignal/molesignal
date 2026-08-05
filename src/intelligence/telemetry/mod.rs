// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Intelligence 模型遥测。
//!
//! 原 infra LLM telemetry 模块全量迁入并改名为 intelligence：
//! - 派生 stream `intelligence_model_traces`（原 `llm_traces`）
//! - HTTP 路径 `/api/v1/intelligence/{stats,top_models,top_users}`（原 `/api/v1/llm/...`）
//! - feature gate：`license.has_feature("intelligence")` → 没有许可证返 403

pub mod fanout;
pub mod redact;
pub mod stats;

pub use fanout::{IntelligenceFanoutHook, extract_batch};
pub use redact::redact_pii;
pub use stats::IntelligenceStatsQuery;

/// 派生 stream 名（全局常量，wire / handler / 测试统一引用）。
pub const INTELLIGENCE_STREAM: &str = "_intelligence_model_traces";

/// License feature 名（用于 `has_feature` 校验）。
pub const INTELLIGENCE_FEATURE: &str = "intelligence";
