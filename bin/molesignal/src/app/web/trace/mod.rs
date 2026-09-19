// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Trace view use case re-exports（web-investigation-shell）。
//!
//! Pure-function 实装在 [`view`]：
//! - `rows_to_spans` 把 traces 流的 `QueryResult` 折叠成 Span 树
//! - `SPAN_LIMIT` / `RESPONSE_HARD_CAP` 截断阈值
//!
//! 该模块仅作命名收口，让 `app::web::{search, topology, correlation, investigation_blob, trace}`
//! 套用一致的 use case 命名。

pub use self::view::{
    RESPONSE_HARD_CAP, SPAN_LIMIT, Span, SpanEvent, TraceResponse, rows_to_spans,
};

pub mod view;
