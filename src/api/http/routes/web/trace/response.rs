// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/api/v1/web/trace/{trace_id}` 响应结构（web-investigation-shell）。
//!
//! Re-export 共享 DTO from [`crate::app::web::trace::view`] —— 同源类型也被
//! intelligence MCP `get_trace` tool 复用（change `intelligence-mcp-dispatcher`）。

pub use crate::app::web::trace::view::{SPAN_LIMIT, TraceResponse};
