// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Serialize;

use super::{ApmQueryRange, ServiceProfile};
use crate::{
    domain::apm::{
        DataQuality, DependencyIdentity, ErrorIdentity, ErrorIssue, ErrorIssueStatus,
        HistogramBucket, QueryResolution, ServiceIdentity, TraceExemplar, TransactionIdentity,
    },
    shared::{ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApmResponseMeta {
    pub range: ApmQueryRange,
    pub resolution: QueryResolution,
    pub projection_started_at: Option<TimestampMicros>,
    pub last_complete_bucket_at: Option<TimestampMicros>,
    pub data_quality: DataQuality,
    pub activation_boundary: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedSummary {
    pub request_count: u64,
    pub error_count: u64,
    pub error_rate: f64,
    pub duration_sum: u64,
    pub duration_average: Option<u64>,
    pub p50_time: Option<u64>,
    pub p90_time: Option<u64>,
    pub p95_time: Option<u64>,
    pub p99_time: Option<u64>,
    /// Satisfied requests and half the tolerating ones over all of them,
    /// against the threshold the summary was read with; a failed request is
    /// never satisfied. `None` without requests.
    pub apdex: Option<f64>,
    pub latency_partial: bool,
    pub exemplars: Vec<TraceExemplar>,
    /// The requests by what they answered (`http:404`, `grpc:14`); none from
    /// buckets written before status codes were counted.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub status_codes: std::collections::BTreeMap<String, u64>,
    /// Requests that ran out of time, and requests sent again.
    #[serde(skip_serializing_if = "is_zero")]
    pub timeout_count: u64,
    #[serde(skip_serializing_if = "is_zero")]
    pub retry_count: u64,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedTrendPoint {
    pub bucket_at: TimestampMicros,
    pub red: RedSummary,
}

/// A bucket of the trend a list draws beside a row.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedSparkPoint {
    pub bucket_at: TimestampMicros,
    pub request_count: u64,
    pub error_count: u64,
    pub p95_time: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
pub struct InstrumentationSummary {
    pub runtime_language: Option<String>,
    pub telemetry_sdk_name: Option<String>,
    pub telemetry_sdk_version: Option<String>,
    pub recent_instance_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSummary {
    pub service: ServiceIdentity,
    pub first_seen_at: TimestampMicros,
    pub last_seen_at: TimestampMicros,
    pub instrumentation: InstrumentationSummary,
    pub versions: Vec<String>,
    /// The version seen most recently, and when it was first seen: the last deploy.
    pub current_version: Option<String>,
    pub current_version_since: Option<TimestampMicros>,
    pub red: RedSummary,
    /// The Apdex threshold T the summary was read with: the service's own, or the default.
    pub apdex_threshold: u64,
    pub sparkline: Vec<RedSparkPoint>,
    pub health: ServiceHealth,
    /// What made the health what it is: each figure past its line.
    pub health_reasons: Vec<HealthReason>,
    pub traces: SignalFilterHandle,
    /// What the service runs as within the range; the catalog and a
    /// service's page carry it, the overview does not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<ServiceProfile>,
}

/// A figure of a service past the line that makes it a warning or critical.
#[derive(Debug, Clone, Serialize)]
pub struct HealthReason {
    /// `error_rate` or `p95`.
    pub metric: &'static str,
    pub level: ServiceHealth,
    pub value: f64,
    pub threshold: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceHealth {
    Healthy,
    Warning,
    Critical,
    NoTraffic,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorSummary {
    pub error: ErrorIdentity,
    pub service: ServiceIdentity,
    pub first_seen_at: TimestampMicros,
    pub last_seen_at: TimestampMicros,
    pub occurrence_count: u64,
    pub representative_message: Option<String>,
    pub red: RedSummary,
    pub traces: SignalFilterHandle,
    /// What is being done about it; `None` while nobody has touched it.
    pub issue: Option<ErrorIssueView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorIssueView {
    pub status: ErrorIssueStatus,
    pub assignee_id: Option<Id>,
    pub team_id: Option<Id>,
    pub resolved_at: Option<TimestampMicros>,
    /// Resolved, and seen again since.
    pub regressed: bool,
    pub updated_by: Id,
    pub updated_at: TimestampMicros,
}

impl ErrorIssueView {
    pub fn of(issue: ErrorIssue, last_seen_at: TimestampMicros) -> Self {
        Self {
            regressed: issue.regressed(last_seen_at),
            status: issue.status,
            assignee_id: issue.assignee_id,
            team_id: issue.team_id,
            resolved_at: issue.resolved_at,
            updated_by: issue.updated_by,
            updated_at: issue.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSummary {
    pub service: ServiceIdentity,
    pub version: String,
    pub first_seen_at: TimestampMicros,
    pub last_seen_at: TimestampMicros,
    pub observation_count: u64,
    /// What the version served in the time in view, where it was read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub red: Option<RedSummary>,
    /// The version against the one of its service first seen before it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<VersionVerdict>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VersionVerdict {
    /// The version it is told against.
    pub previous: String,
    /// `regressed`, `improved`, `neutral` or `insufficient_data`.
    pub status: &'static str,
    /// What got worse: `error_rate`, `latency`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub worse: Vec<&'static str>,
    pub delta: RedDelta,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
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
    /// Services seen in the range, and those of them that served any request.
    pub service_count: u64,
    pub active_service_count: u64,
    /// How many services were in each state, a slot of the range at a time.
    pub health_trend: Vec<HealthTrendPoint>,
    pub rankings: ServiceRankings,
    /// Versions first seen within the range, the newest first.
    pub deployments: Vec<VersionSummary>,
    pub environments: Vec<EnvironmentShare>,
    /// Every environment the services run in, whichever one is in view.
    pub available_environments: Vec<String>,
    pub instrumentation: InstrumentationQuality,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthTrendPoint {
    pub bucket_at: TimestampMicros,
    pub counts: ServiceHealthCounts,
}

/// A service as a ranking lists it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceRank {
    pub service: ServiceIdentity,
    pub health: ServiceHealth,
    pub request_count: u64,
    pub error_rate: f64,
    pub p95_time: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ServiceRankings {
    /// The highest P95 first.
    pub slowest: Vec<ServiceRank>,
    /// The highest error rate first, among services with enough requests to tell.
    pub most_errors: Vec<ServiceRank>,
    /// The most requests first.
    pub busiest: Vec<ServiceRank>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentShare {
    pub environment: String,
    pub service_count: u64,
    pub request_count: u64,
}

/// What the services report about themselves, and what they leave out.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentationQuality {
    pub sdks: Vec<SdkShare>,
    /// Services reporting no `deployment.environment`.
    pub without_environment: u64,
    /// Services reporting no `service.version`.
    pub without_version: u64,
    /// Services reporting no `service.instance.id`.
    pub without_instance: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkShare {
    pub language: Option<String>,
    pub sdk_name: Option<String>,
    pub sdk_version: Option<String>,
    pub service_count: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ServiceHealthCounts {
    pub healthy: u64,
    pub warning: u64,
    pub critical: u64,
    pub no_traffic: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PagedResponse<T> {
    pub meta: ApmResponseMeta,
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub previous_cursor: Option<String>,
    pub has_more: bool,
    pub sort: String,
}

/// A page of error groups, with the occurrences of every group the list
/// keeps (not only the page's) by version, environment and language.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorListResponse {
    #[serde(flatten)]
    pub page: PagedResponse<ErrorSummary>,
    pub breakdown: super::ErrorBreakdown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceDetailResponse {
    pub meta: ApmResponseMeta,
    pub service: ServiceSummary,
    pub red: RedSummary,
    pub trend: Vec<RedTrendPoint>,
    /// How the latencies are spread: the buckets that hold any.
    pub latency_distribution: Vec<HistogramBucket>,
    pub transactions: Vec<TransactionSummary>,
    pub dependencies: Vec<DependencySummary>,
    pub errors: Vec<ErrorSummary>,
    pub versions: Vec<VersionSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionSummary {
    pub service: ServiceIdentity,
    pub version: Option<String>,
    pub transaction: TransactionIdentity,
    pub red: RedSummary,
    /// The Apdex threshold T of the endpoint's service the summary was read with.
    pub apdex_threshold: u64,
    pub total_time: u64,
    pub traces: SignalFilterHandle,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionDetailResponse {
    pub meta: ApmResponseMeta,
    pub transaction: TransactionSummary,
    pub trend: Vec<RedTrendPoint>,
    /// How the latencies are spread: the buckets that hold any.
    pub latency_distribution: Vec<HistogramBucket>,
    pub errors: Vec<ErrorSummary>,
    pub versions: Vec<VersionSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencySummary {
    pub service: ServiceIdentity,
    pub version: Option<String>,
    pub dependency: DependencyIdentity,
    pub red: RedSummary,
    pub total_time: u64,
    pub traces: SignalFilterHandle,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
pub struct ErrorDetailResponse {
    pub meta: ApmResponseMeta,
    pub group: ErrorSummary,
    pub trend: Vec<RedTrendPoint>,
    pub affected_transactions: Vec<TransactionSummary>,
    pub affected_versions: Vec<String>,
    /// Its occurrences by version, environment and language.
    pub breakdown: super::ErrorBreakdown,
    pub representative_stack: Vec<String>,
    pub samples: Vec<ErrorSampleView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSide {
    pub version: String,
    pub sample_count: u64,
    pub red: RedSummary,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedDelta {
    pub request_count_absolute: i64,
    pub request_count_relative: Option<f64>,
    pub error_rate_absolute: f64,
    pub error_rate_relative: Option<f64>,
    pub p95_absolute_time: Option<i64>,
    pub p95_relative: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
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
