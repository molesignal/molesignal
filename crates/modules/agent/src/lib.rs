// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 后端能力边界。
//!
//! `Mole Agent` 是产品模块名，也指与用户交互并执行调查的智能体。
//! 所有模型调用、工具注册、调查、审批和执行能力都从本模块导出，避免再出现多套命名。

pub mod agent {
    pub use crate::*;
}

pub mod domain {
    pub use ::domain::*;
}

pub mod shared {
    pub mod contracts {
        pub use ::contracts::*;
    }

    pub use kernel::{Error, Result, ids, time};
    pub use signals::{http_trace, trace_context, trace_stream};
}

pub mod capabilities;
pub mod chat;
pub mod commands;
pub mod currencies;
pub mod feedback;
pub mod inbound_mcp;
pub mod knowledge;
pub mod mcp;
pub mod memory;
pub mod model;
pub mod network;
pub mod references;
pub mod runs;
pub mod scope;
pub mod skills;
pub mod telemetry;
pub mod titles;
pub mod tool_control;
pub mod tools;
pub mod usage;

pub const PRODUCT_NAME: &str = "Mole Agent";
pub const AGENT_NAME: &str = "Mole Agent";
