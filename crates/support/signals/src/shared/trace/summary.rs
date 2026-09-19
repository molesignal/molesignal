// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Stable storage fields for the one-row-per-trace list projection.
//!
//! The projection is carried by one real span rather than a synthetic span, so
//! trace detail and downstream span processing keep their existing semantics.

pub const TRACE_SUMMARY_MARKER_FIELD: &str = "molesignal.trace.summary";
pub const TRACE_SUMMARY_START_NS_FIELD: &str = "molesignal.trace.start_ns";
pub const TRACE_SUMMARY_DURATION_NS_FIELD: &str = "molesignal.trace.duration_ns";
pub const TRACE_SUMMARY_SPAN_COUNT_FIELD: &str = "molesignal.trace.span_count";
pub const TRACE_SUMMARY_ERROR_COUNT_FIELD: &str = "molesignal.trace.error_count";
