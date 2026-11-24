// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How well each service can be tied to the rest of its data: whether its log
//! lines carry trace ids and whether those traces are kept, whether its spans
//! say where and as what it runs, whether its metrics carry its name, and
//! whether it reports its changes. Also the names logs and metrics give a
//! service that APM does not know, which no page can tie to anything.
//!
//! Logs and metrics are looked at over the last hour of the range, the
//! services over the whole range, changes over the week before its end. Every
//! look is bounded in rows and time; one that fails or runs out of time reads
//! as unknown, never as missing. The report is kept five minutes.

mod grade;
mod looks;

#[cfg(test)]
mod tests;

use std::{sync::LazyLock, time::Duration};

use domain::{
    apm::{ApmCatalogRepository, ServiceIdentity},
    stream::StreamRepository,
};
pub use grade::Grade;
use kernel::{
    Result,
    ids::Id,
    time::{TimeRange, TimestampMicros},
};
pub use looks::{LOG_ERROR_LEVELS, LOG_LEVEL_FIELDS, LOG_SERVICE_FIELDS};
use moka::future::Cache;
use serde::Serialize;

use super::{ApmQueryRequest, ApmQueryService, OrgNames};
use crate::app::query::QueryService;

/// How far back logs and metrics are looked at from the end of the range.
pub const LOOK_WINDOW_MICROS: i64 = 3_600_000_000;
/// How far back changes are counted from the end of the range.
pub const CHANGE_WINDOW_MICROS: i64 = 7 * 86_400_000_000;

static REPORTS: LazyLock<Cache<ReportKey, CorrelationReport>> = LazyLock::new(|| {
    Cache::builder()
        .max_capacity(2_000)
        .time_to_live(Duration::from_secs(300))
        .build()
});

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ReportKey {
    org_id: Id,
    system_streams: bool,
    namespace: Option<String>,
    service: Option<String>,
    environment: Option<String>,
    from_slot: i64,
    to_slot: i64,
}

/// Whether a signal was found, was not, or could not be looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LookState {
    Reported,
    Missing,
    Unknown,
}

/// What the logs say of one service over the look window.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogCorrelation {
    pub state: LookState,
    /// The log streams holding its lines.
    pub streams: Vec<String>,
    pub lines: u64,
    /// Lines carrying a trace id.
    pub traced: u64,
    /// Trace ids taken from its recent lines to look for among the traces, and how many were
    /// found there.
    pub sampled: u64,
    pub matched: u64,
}

/// What a service's spans say about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceCompleteness {
    /// `deployment.environment(.name)`.
    pub environment: bool,
    /// `service.version`.
    pub version: bool,
    /// An instance of it was seen.
    pub instance: bool,
    /// An instance said where it runs: its host, pod, node or container.
    pub placement: bool,
}

/// The changes a service reported over the change window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeCorrelation {
    pub count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_at: Option<TimestampMicros>,
}

/// One grade for each way the service ties to other data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrelationGrades {
    pub trace_ids: Grade,
    pub resource: Grade,
    pub metrics: Grade,
    pub changes: Grade,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceCorrelation {
    pub service: ServiceIdentity,
    pub logs: LogCorrelation,
    pub metrics: LookState,
    pub resource: ResourceCompleteness,
    pub changes: ChangeCorrelation,
    pub grades: CorrelationGrades,
}

/// The signal a name no APM service answers to was found in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NamedIn {
    Logs,
    Metrics,
}

/// A service name in logs or metrics that no APM service answers to, naming rules and
/// aliases applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnmatchedName {
    pub name: String,
    pub signal: NamedIn,
    pub streams: Vec<String>,
    /// Log lines under the name; none for metrics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrelationReport {
    /// The window logs and metrics were looked at over.
    pub from: TimestampMicros,
    pub to: TimestampMicros,
    /// Changes are counted from here to `to`.
    pub changes_since: TimestampMicros,
    pub services: Vec<ServiceCorrelation>,
    pub unmatched: Vec<UnmatchedName>,
}

/// Where a report is read from.
pub struct CorrelationReader<'a> {
    pub query: &'a QueryService,
    pub streams: &'a dyn StreamRepository,
    pub apm: &'a ApmQueryService,
    pub catalog: &'a dyn ApmCatalogRepository,
}

impl CorrelationReader<'_> {
    /// The report for the services in `request`'s range, namespace and environment. The
    /// platform's own log stream is read only when `system_streams` is set.
    pub async fn report(
        &self,
        org_id: &Id,
        request: ApmQueryRequest,
        system_streams: bool,
    ) -> Result<CorrelationReport> {
        let key = ReportKey {
            org_id: org_id.clone(),
            system_streams,
            namespace: request.namespace.clone(),
            service: request.service.clone(),
            environment: request.environment.clone(),
            from_slot: request.from / 300_000_000,
            to_slot: request.to / 300_000_000,
        };

        if let Some(kept) = REPORTS.get(&key).await {
            return Ok(kept);
        }

        let to = TimestampMicros(request.to);
        let window = TimeRange::new(TimestampMicros(request.to - LOOK_WINDOW_MICROS), to);
        let changes_since = TimestampMicros(request.to - CHANGE_WINDOW_MICROS);

        let context = self
            .apm
            .context(org_id.clone(), request, &["name"], "name")?;
        let names = OrgNames::of(self.apm.org_settings(org_id).await.as_ref());

        let looker = looks::Looker {
            query: self.query,
            org_id,
            window,
            names: &names,
        };
        let streams = self.streams.list(org_id).await?;

        let (inventory, changes, logs, metrics) = tokio::join!(
            self.apm.inventory(&context),
            self.catalog.change_counts(org_id, changes_since),
            looker.logs(&streams, system_streams),
            looker.metrics(&streams),
        );

        let report = grade::assemble(grade::Inputs {
            window,
            changes_since,
            inventory: inventory?,
            // Without the changes every service would read as reporting none.
            changes: changes.ok(),
            logs,
            metrics,
        });

        REPORTS.insert(key, report.clone()).await;
        Ok(report)
    }
}
