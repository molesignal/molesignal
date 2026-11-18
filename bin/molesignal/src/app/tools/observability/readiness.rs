// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `check_data_readiness`: how ready the organization's telemetry is for an investigation.
//!
//! Each stream of logs, traces and metrics is read with two bounded queries (what its rows carry,
//! and which services and environments they name), a few streams at a time, and the whole check
//! stops at [`BUDGET`]. The names found are then held against each other across the streams:
//! services written in more than one way, services seen in one signal only, environments spelled
//! in several ways, and metric names that do not say their unit.

mod fields;
mod findings;
mod names;
mod probe;

use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use domain::{
    iam::IamContext,
    stream::{StreamDefinition, StreamType},
};
use futures::future::join_all;
use kernel::{
    Error, Result,
    ids::Id,
    time::{TimeRange, TimestampMicros},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tool_runtime::ToolResult;

use self::{
    findings::{
        Finding, Severity, environment_findings, listed, mixed_service_fields, near_miss_findings,
        one_signal_findings, stream_findings, unit_findings,
    },
    names::{Signal, UnitGap, environment_spellings, near_misses, only_in_one_signal, unit_gap},
    probe::{Probe, probe},
};
use super::super::{ToolRuntime, common::*};
use crate::app::apm::ApmQueryRequest;

/// The most streams checked at once.
const MAX_STREAMS: usize = 50;

/// Streams read at the same time.
const CONCURRENCY: usize = 4;

/// The whole check stops after this long, within the time a read-only tool is given.
const BUDGET: Duration = Duration::from_secs(22);

/// The range read when none is asked for.
const DEFAULT_RANGE_MICROS: i64 = 24 * 60 * 60 * 1_000_000;

/// The most findings told.
const MAX_FINDINGS: usize = 100;

/// The most services told of one stream.
const SERVICES_PER_STREAM: usize = 5;

/// The most APM services read.
const APM_SERVICES: usize = 100;

/// APM is asked for its services for no longer than this.
const APM_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Args {
    #[serde(default)]
    streams: Vec<String>,
    #[serde(default)]
    time_range: Option<TimeRangeArg>,
}

pub(super) async fn check(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: Args = parse_args(arguments)?;

    if args.streams.len() > MAX_STREAMS {
        return Err(Error::invalid(format!(
            "streams names at most {MAX_STREAMS} streams"
        )));
    }

    let range = match args.time_range {
        Some(range) => time_range(range)?,
        None => {
            let end = TimestampMicros::now();
            TimeRange::new(TimestampMicros(end.0 - DEFAULT_RANGE_MICROS), end)
        }
    };

    let deadline = Instant::now() + BUDGET;
    let (streams, mut found) = chosen_streams(runtime, &auth.org_id, &args.streams).await?;
    let mut probes = Vec::new();

    for chunk in streams.chunks(CONCURRENCY) {
        probes.extend(
            join_all(chunk.iter().map(|(stream, signal)| {
                probe(runtime, &auth.org_id, stream, *signal, range, deadline)
            }))
            .await,
        );
    }

    let metrics = metric_names(runtime, &auth.org_id).await?;
    let apm = apm_services(runtime, &auth.org_id, range, deadline).await;

    found.extend(assess(&probes, &metrics, &apm));

    Ok(ToolResult::json(report(range, &probes, found)))
}

/// The streams to check, logs first, then metrics and traces, and findings for the names asked
/// for that are not streams one may read.
async fn chosen_streams(
    runtime: &ToolRuntime,
    org_id: &Id,
    asked: &[String],
) -> Result<(Vec<(StreamDefinition, Signal)>, Vec<Finding>)> {
    let mut chosen = Vec::new();

    for stream in runtime.observability.streams.list(org_id).await? {
        let Some(signal) = signal_of(stream.stream_type) else {
            continue;
        };

        let wanted = if asked.is_empty() {
            !is_internal_stream(&stream.name)
        } else {
            asked.contains(&stream.name)
        };

        // A tool call carries nobody's audit.read: the audit trail is never read here.
        if !wanted || !crate::infra::query::grants::may_read(&stream.name) {
            continue;
        }

        if runtime
            .observability
            .streams
            .get_settings(&stream.id)
            .await?
            .queryable
        {
            chosen.push((stream, signal));
        }
    }

    chosen.sort_by(|(left, left_signal), (right, right_signal)| {
        left_signal
            .cmp(right_signal)
            .then_with(|| left.name.cmp(&right.name))
    });

    let mut found = asked
        .iter()
        .filter(|name| !chosen.iter().any(|(stream, _)| &stream.name == *name))
        .map(|name| {
            Finding::new(
                Severity::Warning,
                "stream_not_found",
                format!("`{name}` is not a stream of logs, traces or metrics that can be queried."),
                "List the streams with list_streams and check the name.",
            )
        })
        .collect::<Vec<_>>();

    if chosen.len() > MAX_STREAMS {
        let rest = chosen
            .split_off(MAX_STREAMS)
            .into_iter()
            .map(|(stream, _)| stream.name)
            .collect::<Vec<_>>();

        found.push(Finding::new(
            Severity::Info,
            "not_checked",
            format!(
                "Only {MAX_STREAMS} streams are checked at once; these were not: {}.",
                listed(&rest)
            ),
            "Check them by naming them in streams.",
        ));
    }

    Ok((chosen, found))
}

/// What a stream holds, as a signal; streams of other kinds are not checked.
fn signal_of(stream_type: StreamType) -> Option<Signal> {
    if stream_type == StreamType::LOGS {
        Some(Signal::Logs)
    } else if stream_type == StreamType::TRACES {
        Some(Signal::Traces)
    } else if stream_type == StreamType::METRICS {
        Some(Signal::Metrics)
    } else {
        None
    }
}

/// The metric names of the organization with their kind, each once.
async fn metric_names(
    runtime: &ToolRuntime,
    org_id: &Id,
) -> Result<BTreeMap<String, Option<String>>> {
    Ok(runtime
        .observability
        .metric_directory
        .list(org_id)
        .await?
        .into_iter()
        .filter(|entry| !is_internal_stream(&entry.stream_name))
        .map(|entry| (entry.metric_name, entry.metric_kind))
        .collect())
}

/// The services APM saw in the range, with their environment; nothing when APM cannot answer
/// before `deadline`.
async fn apm_services(
    runtime: &ToolRuntime,
    org_id: &Id,
    range: TimeRange,
    deadline: Instant,
) -> Vec<(String, String)> {
    let left = deadline
        .saturating_duration_since(Instant::now())
        .min(APM_TIMEOUT);

    if left.is_zero() {
        return Vec::new();
    }

    let request = ApmQueryRequest {
        from: range.start.0,
        to: range.end.0,
        limit: Some(APM_SERVICES),
        ..ApmQueryRequest::default()
    };

    let apm = &runtime.observability.apm;

    let Ok(context) = apm.context(org_id.clone(), request, &["request_count"], "request_count")
    else {
        return Vec::new();
    };

    match tokio::time::timeout(left, apm.services(&context)).await {
        Ok(Ok(page)) => page
            .items
            .into_iter()
            .map(|summary| (summary.service.name, summary.service.environment))
            .collect(),
        Ok(Err(error)) => {
            tracing::debug!(%error, "APM services could not be read for the readiness check");
            Vec::new()
        }
        Err(_) => Vec::new(),
    }
}

/// What stands in the way of an investigation: in each stream, then across them.
fn assess(
    probes: &[Probe],
    metrics: &BTreeMap<String, Option<String>>,
    apm: &[(String, String)],
) -> Vec<Finding> {
    let mut found = probes.iter().flat_map(stream_findings).collect::<Vec<_>>();

    let unread = probes
        .iter()
        .filter(|probe| probe.coverage.is_none())
        .map(|probe| probe.name.clone())
        .collect::<Vec<_>>();

    if !unread.is_empty() {
        let why = probes
            .iter()
            .find_map(|probe| probe.error.as_deref())
            .unwrap_or_default();

        found.push(Finding::new(
            Severity::Info,
            "not_checked",
            format!(
                "These streams could not be read ({why}): {}.",
                listed(&unread)
            ),
            "Run the check again with fewer streams or a shorter range.",
        ));
    }

    found.extend(mixed_service_fields(probes));

    let mut services = BTreeMap::<String, BTreeSet<Signal>>::new();
    let mut environments = BTreeMap::<String, u64>::new();

    for probe in probes {
        for value in &probe.values {
            if let Some(service) = &value.service {
                services
                    .entry(service.clone())
                    .or_default()
                    .insert(probe.signal);
            }

            if let Some(environment) = &value.environment {
                *environments.entry(environment.clone()).or_default() += value.count;
            }
        }
    }

    for (service, environment) in apm {
        services
            .entry(service.clone())
            .or_default()
            .insert(Signal::Traces);

        if !environment.trim().is_empty() {
            *environments.entry(environment.clone()).or_default() += 1;
        }
    }

    found.extend(near_miss_findings(&near_misses(&services)));
    found.extend(one_signal_findings(&only_in_one_signal(&services)));
    found.extend(environment_findings(&environment_spellings(&environments)));

    let mut gaps = BTreeMap::<UnitGap, Vec<String>>::new();

    for (metric, kind) in metrics {
        if let Some(gap) = unit_gap(metric, kind.as_deref()) {
            gaps.entry(gap).or_default().push(metric.clone());
        }
    }

    found.extend(unit_findings(&gaps));
    found
}

fn report(range: TimeRange, probes: &[Probe], mut found: Vec<Finding>) -> Value {
    found.sort_by(|left, right| {
        left.severity
            .cmp(&right.severity)
            .then_with(|| left.stream.cmp(&right.stream))
    });

    let count = |severity: Severity| {
        found
            .iter()
            .filter(|finding| finding.severity == severity)
            .count()
    };

    let summary = json!({
        "critical": count(Severity::Critical),
        "warning": count(Severity::Warning),
        "info": count(Severity::Info),
    });

    let total = found.len();
    found.truncate(MAX_FINDINGS);

    let mut out = Map::new();

    out.insert(
        "timeRange".into(),
        json!({"startTime": range.start.0, "endTime": range.end.0}),
    );

    out.insert(
        "checkedStreams".into(),
        json!(
            probes
                .iter()
                .filter(|probe| probe.coverage.is_some())
                .count()
        ),
    );

    out.insert("summary".into(), summary);
    out.insert("findings".into(), json!(found));

    if total > MAX_FINDINGS {
        out.insert("findingsLeftOut".into(), json!(total - MAX_FINDINGS));
    }

    out.insert(
        "streams".into(),
        Value::Array(probes.iter().map(stream_summary).collect()),
    );

    Value::Object(out)
}

/// What one stream records, for the report.
fn stream_summary(probe: &Probe) -> Value {
    let detected = &probe.detected;
    let mut out = Map::new();

    out.insert("name".into(), json!(probe.name));
    out.insert("signal".into(), json!(probe.signal.as_str()));

    let Some(coverage) = &probe.coverage else {
        out.insert("checked".into(), json!(false));
        out.insert("reason".into(), json!(probe.error));
        return Value::Object(out);
    };

    let share = |part: u64| {
        if coverage.total == 0 {
            Value::Null
        } else {
            json!(((part as f64 / coverage.total as f64) * 100.0).round() / 100.0)
        }
    };

    let field = |name: &Option<String>, part: u64| {
        name.as_ref().map_or(
            Value::Null,
            |name| json!({"field": name, "coverage": share(part)}),
        )
    };

    out.insert("checked".into(), json!(true));
    out.insert("rows".into(), json!(coverage.total));
    out.insert("service".into(), field(&detected.service, coverage.service));

    out.insert(
        "environment".into(),
        field(&detected.environment, coverage.environment),
    );

    if probe.signal == Signal::Logs {
        out.insert(
            "traceId".into(),
            field(&detected.trace_id, coverage.trace_id),
        );
        out.insert("messageField".into(), json!(detected.message));
    }

    let mut by_service = BTreeMap::<&str, u64>::new();

    for value in &probe.values {
        if let Some(service) = &value.service {
            *by_service.entry(service.as_str()).or_default() += value.count;
        }
    }

    let mut by_service = by_service.into_iter().collect::<Vec<_>>();
    by_service.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(right.0)));

    out.insert(
        "services".into(),
        json!(
            by_service
                .into_iter()
                .take(SERVICES_PER_STREAM)
                .map(|(service, _)| service)
                .collect::<Vec<_>>()
        ),
    );

    if let Some(error) = &probe.error {
        out.insert("partial".into(), json!(error));
    }

    Value::Object(out)
}

#[cfg(test)]
mod tests;
