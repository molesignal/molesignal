// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Process signals: logs, metrics, tracing context, self-telemetry, and tail sampling.

pub mod policy;

pub mod domain {
    pub use ::domain::*;
}

pub mod shared {
    pub mod metrics;
    pub mod self_telemetry;
    pub mod tail_sampling;
    pub mod telemetry;
    pub mod trace;

    pub use kernel::{Error, Result, ids, time};
    pub use trace::{
        context as trace_context, coverage as trace_coverage, fixtures as trace_fixtures,
        grpc as grpc_trace, http as http_trace, metrics as trace_metrics,
        normalization as trace_normalization, stream as trace_stream,
    };
}

pub use shared::{
    grpc_trace, http_trace, metrics, self_telemetry, tail_sampling, telemetry, trace,
    trace_context, trace_coverage, trace_fixtures, trace_metrics, trace_normalization,
    trace_stream,
};
