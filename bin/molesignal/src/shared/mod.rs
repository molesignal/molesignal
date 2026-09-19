// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

pub mod build_info;

pub use signals::{
    grpc_trace, http_trace, metrics, self_telemetry, tail_sampling, telemetry, trace,
    trace_context, trace_coverage, trace_fixtures, trace_metrics, trace_normalization,
    trace_stream,
};

pub mod contracts {
    pub use ::contracts::*;
}

pub use ::report_renderer::{
    RenderError, ReportFormat, ReportRenderer, Viewport, validate_report_bytes,
};
pub use kernel::{
    CommunityLicense, Error, LicenseGate, LicenseHolder, Probe, Result, cursor, drain, error,
    health, ids, license, time,
};
