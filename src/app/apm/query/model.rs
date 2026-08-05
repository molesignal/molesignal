// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Serialize;

use super::ApmQueryRange;
use crate::{
    domain::apm::{
        DataQuality, DependencyIdentity, ErrorIdentity, QueryResolution, ServiceIdentity,
        TraceExemplar, TransactionIdentity,
    },
    shared::time::TimestampMicros,
};

#[derive(Debug, Clone, Serialize)]
pub struct ApmResponseMeta {
    pub range: ApmQueryRange,
    pub resolution: QueryResolution,
    pub projection_started_at: Option<TimestampMicros>,
    pub last_complete_bucket_at: Option<TimestampMicros>,
    pub data_quality: DataQuality,
    pub activation_boundary: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RedSummary {
    pub request_count: u64,
    pub error_count: u64,
    pub error_rate: f64,
    pub duration_sum_micros: u64,
    pub duration_average_micros: Option<u64>,
    pub p50_micros: Option<u64>,
    pub p95_micros: Option<u64>,
    pub p99_micros: Option<u64>,
    pub latency_partial: bool,
    pub exemplars: Vec<TraceExemplar>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RedTrendPoint {
    pub bucket_at: TimestampMicros,
    pub red: RedSummary,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignalFilterHandle {
    pub namespace: String,
    pub service: String,
    pub environment: String,
    pub version: Option<String>,
    pub transaction: Option<String>,
    pub dependency: Option<String>,
    pub error_fingerprint: Option<String>,
    pub from: TimestampMicros,
    pub to: TimestampMicros,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstrumentationSummary {
    pub runtime_language: Option<String>,
    pub telemetry_sdk_name: Option<String>,
    pub telemetry_sdk_version: Option<String>,
    pub recent_instance_count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceSummary {
    pub service: ServiceIdentity,
    pub first_seen_at: TimestampMicros,
    pub last_seen_at: TimestampMicros,
    pub instrumentation: InstrumentationSummary,
    pub versions: Vec<String>,
    pub red: RedSummary,
    pub health: ServiceHealth,
    pub traces: SignalFilterHandle,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceHealth {
    Healthy,
    Warning,
    Critical,
    NoTraffic,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorSummary {
    pub error: ErrorIdentity,
    pub service: ServiceIdentity,
    pub first_seen_at: TimestampMicros,
    pub last_seen_at: TimestampMicros,
    pub occurrence_count: u64,
    pub representative_message: Option<String>,
    pub red: RedSummary,
    pub traces: SignalFilterHandle,
}

#[derive(Debug, Clone, Serialize)]
pub struct VersionSummary {
    pub service: ServiceIdentity,
    pub version: String,
    pub first_seen_at: TimestampMicros,
    pub last_seen_at: TimestampMicros,
    pub observation_count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct OverviewResponse {
    pub meta: ApmResponseMeta,
    pub red: RedSummary,
    pub trend: Vec<RedTrendPoint>,
    pub service_health: ServiceHealthCounts,
    pub services: Vec<ServiceSummary>,
    pub top_transactions: Vec<TransactionSummary>,
    pub top_dependencies: Vec<DependencySummary>,
    pub top_errors: Vec<ErrorSummary>,
    pub recent_versions: Vec<VersionSummary>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ServiceHealthCounts {
    pub healthy: u64,
    pub warning: u64,
    pub critical: u64,
    pub no_traffic: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PagedResponse<T> {
    pub meta: ApmResponseMeta,
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub previous_cursor: Option<String>,
    pub has_more: bool,
    pub sort: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceDetailResponse {
    pub meta: ApmResponseMeta,
    pub service: ServiceSummary,
    pub red: RedSummary,
    pub trend: Vec<RedTrendPoint>,
    pub transactions: Vec<TransactionSummary>,
    pub dependencies: Vec<DependencySummary>,
    pub errors: Vec<ErrorSummary>,
    pub versions: Vec<VersionSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransactionSummary {
    pub service: ServiceIdentity,
    pub version: Option<String>,
    pub transaction: TransactionIdentity,
    pub red: RedSummary,
    pub total_time_micros: u64,
    pub traces: SignalFilterHandle,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransactionDetailResponse {
    pub meta: ApmResponseMeta,
    pub transaction: TransactionSummary,
    pub trend: Vec<RedTrendPoint>,
    pub errors: Vec<ErrorSummary>,
    pub versions: Vec<VersionSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DependencySummary {
    pub service: ServiceIdentity,
    pub version: Option<String>,
    pub dependency: DependencyIdentity,
    pub red: RedSummary,
    pub total_time_micros: u64,
    pub traces: SignalFilterHandle,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorSampleView {
    pub event_time: TimestampMicros,
    pub trace_id: String,
    pub span_id: String,
    pub trace_available: bool,
    pub trace_link: Option<String>,
    pub representative_message: Option<String>,
    pub representative_stack: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorDetailResponse {
    pub meta: ApmResponseMeta,
    pub group: ErrorSummary,
    pub trend: Vec<RedTrendPoint>,
    pub affected_transactions: Vec<TransactionSummary>,
    pub affected_versions: Vec<String>,
    pub representative_stack: Vec<String>,
    pub samples: Vec<ErrorSampleView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VersionSide {
    pub version: String,
    pub sample_count: u64,
    pub red: RedSummary,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RedDelta {
    pub request_count_absolute: i64,
    pub request_count_relative: Option<f64>,
    pub error_rate_absolute: f64,
    pub error_rate_relative: Option<f64>,
    pub p95_absolute_micros: Option<i64>,
    pub p95_relative: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VersionCompareResponse {
    pub meta: ApmResponseMeta,
    pub baseline: VersionSide,
    pub candidate: VersionSide,
    pub sufficient_data: bool,
    pub status: &'static str,
    pub delta: RedDelta,
    pub regressed_transactions: Vec<TransactionSummary>,
    pub regressed_errors: Vec<ErrorSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApmTenantHealthResponse {
    pub meta: ApmResponseMeta,
    pub enabled: bool,
    pub degraded: bool,
    pub runtime: Option<crate::app::apm::ApmRuntimeHealthSnapshot>,
}
