// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What insights say: each finding, the figures behind it, and the changes
//! near when it began.

use serde::Serialize;

use super::super::{ApmResponseMeta, RedSummary};
use crate::{
    domain::apm::{
        ChangeKind, DependencyIdentity, DeploymentStatus, ServiceIdentity, TransactionIdentity,
    },
    shared::time::TimestampMicros,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InsightKind {
    /// Errors, latency or traffic moved against the stretch before and the
    /// same stretch a week before.
    Anomaly,
    /// A version first seen in the range did worse than the one before it.
    Regression,
    /// A dependency takes most of the time of the service's requests.
    SlowDependency,
    /// The same call made many times a request: an N+1 pattern.
    RepeatedCall,
    /// A database statement slow enough to look at.
    SlowQuery,
    /// The endpoint where most of the service's time goes.
    HotEndpoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InsightSeverity {
    Critical,
    Warning,
    /// A suggestion: worth a look, nothing is wrong yet.
    Info,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Insight {
    pub kind: InsightKind,
    pub severity: InsightSeverity,
    pub service: ServiceIdentity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<TransactionIdentity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependency: Option<DependencyIdentity>,
    /// What moved or got worse: `error_rate`, `p95` or `throughput`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metric: Option<&'static str>,
    /// The figure now: the error rate, the P95 in microseconds, the
    /// requests a second; a share of time; the calls a request.
    pub value: f64,
    /// The same figure over the stretch of the same length before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous: Option<f64>,
    /// The same figure over the same stretch a week before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_week: Option<f64>,
    /// When the move began, as the buckets of the range show it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<TimestampMicros>,
    /// A regression's version, and the version before it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_version: Option<String>,
    /// The calls or requests a suggestion rests on, and their P95.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calls: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p95_time: Option<u64>,
    /// Changes near when it began, the nearest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<NearbyChange>,
}

impl Insight {
    pub(super) fn new(
        kind: InsightKind,
        severity: InsightSeverity,
        service: ServiceIdentity,
        value: f64,
    ) -> Self {
        Self {
            kind,
            severity,
            service,
            transaction: None,
            dependency: None,
            metric: None,
            value,
            previous: None,
            last_week: None,
            since: None,
            version: None,
            previous_version: None,
            calls: None,
            p95_time: None,
            changes: Vec::new(),
        }
    }
}

/// Where a change is known from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeSource {
    /// A pipeline or a tool reported it.
    Reported,
    /// The spans first carried the version then.
    Observed,
}

#[derive(Debug, Clone, Serialize)]
pub struct NearbyChange {
    pub kind: ChangeKind,
    pub source: ChangeSource,
    pub service: String,
    /// The version deployed; empty for a change that names none.
    pub version: String,
    pub status: DeploymentStatus,
    pub at: TimestampMicros,
    /// A service the moving one calls, rather than itself.
    pub downstream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// The services in view over a stretch, all together.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BaselineFigures {
    pub request_count: u64,
    pub throughput: f64,
    pub error_rate: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p95_time: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apdex: Option<f64>,
}

impl BaselineFigures {
    /// `red` over `seconds`, its count told for `share` of the stretch read.
    pub(super) fn of(red: &RedSummary, seconds: f64, share: f64) -> Self {
        Self {
            request_count: (red.request_count as f64 * share).round() as u64,
            throughput: red.request_count as f64 / seconds.max(1e-9),
            error_rate: red.error_rate,
            p95_time: red.p95_time,
            apdex: red.apdex,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InsightsResponse {
    pub meta: ApmResponseMeta,
    /// The services in view now, over the stretch before, and over the same
    /// stretch a week before; a stretch without requests is absent.
    pub now: BaselineFigures,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous: Option<BaselineFigures>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_week: Option<BaselineFigures>,
    /// The most pressing first.
    pub insights: Vec<Insight>,
}
