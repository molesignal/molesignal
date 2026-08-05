// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

pub mod build_info;
pub mod contracts;
pub mod cursor;
pub mod drain;
pub mod error;
pub mod health;
pub mod ids;
pub mod license;
pub mod metrics;
pub mod report_renderer;
pub mod self_telemetry;
pub mod tail_sampling;
pub mod telemetry;
pub mod time;
pub mod trace;

pub use error::{Error, Result};
pub use health::Probe;
pub use license::{CommunityLicense, LicenseGate, LicenseHolder};
pub use report_renderer::{
    RenderError, ReportFormat, ReportRenderer, Viewport, validate_report_bytes,
};
pub use trace::{
    context as trace_context, coverage as trace_coverage, fixtures as trace_fixtures,
    grpc as grpc_trace, http as http_trace, metrics as trace_metrics,
    normalization as trace_normalization, stream as trace_stream,
};
