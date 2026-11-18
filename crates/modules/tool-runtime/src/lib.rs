// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Protocol-neutral contract for MoleSignal platform tools.
//!
//! This crate does not depend on HTTP, a model provider, or the application composition root.
//! Mole Agent, the future inbound MCP Server, and automation entry points all reuse the same
//! catalog, call context, and result types.

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
