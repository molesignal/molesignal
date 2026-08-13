// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MoleSignal 平台工具的协议中立契约。
//!
//! 本 crate 不依赖 HTTP、模型供应商或应用 composition root。Mole Agent、未来的入站
//! MCP Server，以及自动化入口都复用同一份 catalog、调用上下文和结果类型。

mod context;
mod result;
mod spec;

pub mod catalog;

pub use context::{
    ExecutionPolicy, ToolCallSource, ToolDispatcher, ToolExecutionMode, ToolInvocationContext,
};
pub use result::{ToolCall, ToolContent, ToolResult};
pub use spec::{
    PermissionMode, RiskLevel, ToolAccess, ToolAnnotations, ToolExposure, ToolSpec, ToolSurface,
};
