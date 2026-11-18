// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent model telemetry.
//!
//! The original infra LLM telemetry module moves here in full and is renamed to agent:
//! - Derived stream `agent_model_traces` (formerly `llm_traces`)
//! - HTTP path `/api/v1/agent/{stats,top_models,top_users}` (formerly `/api/v1/llm/...`)

pub mod fanout;
pub mod redact;
pub mod stats;

pub use fanout::{AgentFanoutHook, extract_batch};
pub use redact::redact_pii;
pub use stats::{AgentStatsQuery, OptionalColumns};

/// The derived stream name (a global constant shared by wire / handler / tests).
pub const AGENT_STREAM: &str = "_agent_model_traces";
