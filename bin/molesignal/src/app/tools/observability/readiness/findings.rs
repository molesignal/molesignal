// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What stands in the way of an investigation, with what to do about it: per stream, and across
//! the streams.

use std::collections::BTreeMap;

use serde::Serialize;

use super::{
    fields::is_conventional_service_field,
    names::{NearMiss, Signal, UnitGap},
    probe::Probe,
};

/// Below this share of rows naming their service, the rest are called out.
const SERVICE_COVERAGE: f64 = 0.9;

/// Below this share of logs carrying a trace id, they are called out.
const TRACE_COVERAGE: f64 = 0.5;

/// Above this share of messages left as JSON text, they are called out.
const JSON_MESSAGES: f64 = 0.3;

/// A log stream with no more fields of its own than this holds little but its message.
const UNSTRUCTURED_FIELDS: usize = 2;

/// The most names listed in one finding.
const LISTED: usize = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Severity {
    Critical,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Finding {
    pub(super) severity: Severity,
    pub(super) kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) stream: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) field: Option<String>,
    pub(super) detail: String,
    pub(super) suggestion: String,
}

impl Finding {
    pub(super) fn new(
        severity: Severity,
        kind: &'static str,
        detail: String,
        suggestion: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            kind,
            stream: None,
            field: None,
            detail,
            suggestion: suggestion.into(),
        }
    }

    fn on(mut self, stream: &str, field: Option<&str>) -> Self {
        self.stream = Some(stream.to_string());
        self.field = field.map(str::to_string);
        self
    }
}

/// A field as a VRL path: `.service_name`, or `."service.name"` when the name needs quotes.
fn vrl_path(field: &str) -> String {
    if field
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        format!(".{field}")
    } else {
        format!(".\"{}\"", field.replace('"', "\\\""))
    }
}

fn percent(part: u64, total: u64) -> u64 {
    (part * 100).checked_div(total).unwrap_or(0)
}

fn share(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 / total as f64
    }
}

/// Whether a time written as text says its zone: `Z`, an offset, or UTC/GMT at the end.
fn has_zone(text: &str) -> bool {
    let upper = text.trim().to_uppercase();
    let bytes = upper.as_bytes();

    if upper.ends_with('Z') || upper.ends_with("UTC") || upper.ends_with("GMT") {
        return true;
    }

    // `+08:00` or `+0800` at the end.
    let offset = |length: usize| {
        bytes.len() > length
            && matches!(bytes[bytes.len() - length], b'+' | b'-')
            && bytes[bytes.len() - length + 1..]
                .iter()
                .all(|byte| byte.is_ascii_digit() || *byte == b':')
    };

    offset(6) || offset(5)
}

/// Whether text reads as a moment of the day: digits with a colon, not a bare number.
fn looks_like_a_moment(text: &str) -> bool {
    text.contains(':') && text.bytes().any(|byte| byte.is_ascii_digit())
}

/// What stands in the way of an investigation in one stream.
pub(super) fn stream_findings(probe: &Probe) -> Vec<Finding> {
    let Some(coverage) = &probe.coverage else {
        return Vec::new();
    };

    let name = probe.name.as_str();
    let detected = &probe.detected;
    let mut found = Vec::new();

    if coverage.total == 0 {
        found.push(
            Finding::new(
                Severity::Info,
                "no_data",
                "The stream has no rows in the range, so nothing in it was checked.".into(),
                "Check a range in which the stream receives data.",
            )
            .on(name, None),
        );

        return found;
    }

    found.extend(service_findings(probe));

    if detected.environment.is_none() {
        found.push(
            Finding::new(
                Severity::Info,
                "no_environment_field",
                "No field names the environment, so data of production and of other environments \
                 mix in an investigation."
                    .into(),
                "Record the environment on every row, as deployment.environment (OpenTelemetry) \
                 or env.",
            )
            .on(name, None),
        );
    }

    if probe.signal == Signal::Logs {
        found.extend(log_findings(probe));
    }

    if let (Some(field), Some(sample)) = (&detected.text_time, &coverage.time_sample)
        && looks_like_a_moment(sample)
        && !has_zone(sample)
    {
        found.push(
            Finding::new(
                Severity::Warning,
                "time_without_zone",
                format!(
                    "`{field}` is written without a time zone (for example `{sample}`), so \
                     moments from different sources may not line up."
                ),
                "Write times in RFC 3339 with an offset or Z at the source, or read them with \
                 their zone in the pipeline (VRL parse_timestamp with its timezone argument).",
            )
            .on(name, Some(field.as_str())),
        );
    }

    found
}

fn service_findings(probe: &Probe) -> Vec<Finding> {
    let name = probe.name.as_str();
    let metrics = probe.signal == Signal::Metrics;
    let mut found = Vec::new();

    let Some(field) = &probe.detected.service else {
        let (severity, suggestion) = if metrics {
            (
                Severity::Warning,
                "Add a service_name label at the source or in the scrape configuration (relabel \
                 job into service_name).",
            )
        } else {
            (
                Severity::Critical,
                "Set the OpenTelemetry resource attribute service.name at the source, or add a \
                 pipeline function that sets service_name from the field that names the service \
                 (VRL: .service_name = .kubernetes.labels.app).",
            )
        };

        found.push(
            Finding::new(
                severity,
                "no_service_field",
                format!(
                    "No field names the service of these {}: an investigation cannot tell which \
                     service a row belongs to.",
                    probe.signal.as_str()
                ),
                suggestion,
            )
            .on(name, None),
        );

        return found;
    };

    if !is_conventional_service_field(field) {
        found.push(
            Finding::new(
                Severity::Info,
                "service_field_synonym",
                format!("The service is recorded as `{field}`."),
                format!(
                    "Rename it to service_name in the stream's pipeline (VRL: .service_name = \
                     del({})) so every signal names the service under one key.",
                    vrl_path(field)
                ),
            )
            .on(name, Some(field.as_str())),
        );
    }

    if let Some(coverage) = &probe.coverage
        && share(coverage.service, coverage.total) < SERVICE_COVERAGE
    {
        found.push(
            Finding::new(
                Severity::Warning,
                "service_coverage",
                format!(
                    "{}% of the rows carry no `{field}`.",
                    100 - percent(coverage.service, coverage.total)
                ),
                "Find the sources that leave it out (group the rows without it by host or \
                 source) and set it there, or give it a default in the pipeline.",
            )
            .on(name, Some(field.as_str())),
        );
    }

    found
}

fn log_findings(probe: &Probe) -> Vec<Finding> {
    let Some(coverage) = &probe.coverage else {
        return Vec::new();
    };

    let name = probe.name.as_str();
    let detected = &probe.detected;
    let mut found = Vec::new();

    match &detected.trace_id {
        None => found.push(
            Finding::new(
                Severity::Warning,
                "no_trace_id",
                "The logs carry no trace id, so a log cannot be followed to its trace.".into(),
                "Write the trace id into every log record (the OpenTelemetry log bridge or the \
                 logging library's trace context does it) and keep it as trace_id.",
            )
            .on(name, None),
        ),
        Some(field) if share(coverage.trace_id, coverage.total) < TRACE_COVERAGE => found.push(
            Finding::new(
                Severity::Info,
                "trace_id_coverage",
                format!(
                    "Only {}% of the logs carry `{field}`.",
                    percent(coverage.trace_id, coverage.total)
                ),
                "Logs written outside a request carry none; when request logs lack it too, the \
                 logging library does not read the trace context.",
            )
            .on(name, Some(field.as_str())),
        ),
        Some(_) => {}
    }

    if let Some(message) = &detected.message {
        if detected.message_is_text && share(coverage.json_messages, coverage.total) > JSON_MESSAGES
        {
            found.push(
                Finding::new(
                    Severity::Warning,
                    "unparsed_json",
                    format!(
                        "{}% of the messages in `{message}` are JSON left as text: their fields \
                         cannot be filtered or grouped.",
                        percent(coverage.json_messages, coverage.total)
                    ),
                    format!(
                        "Parse them at intake with a pipeline function (VRL: . = merge(., \
                         object!(parse_json!({})))).",
                        vrl_path(message)
                    ),
                )
                .on(name, Some(message.as_str())),
            );
        }

        if detected.own_fields <= UNSTRUCTURED_FIELDS {
            found.push(
                Finding::new(
                    Severity::Info,
                    "unstructured_logs",
                    format!(
                        "The stream has {} fields of its own: the level, the service and the \
                         request live inside the message text.",
                        detected.own_fields
                    ),
                    "Write JSON logs, or parse the message at intake (parse_regex or \
                     parse_key_value in a pipeline function); one wide log line per request with \
                     its route, status, duration and trace id answers most questions alone.",
                )
                .on(name, Some(message.as_str())),
            );
        }
    }

    found
}

/// Streams of logs and traces that record the service under different keys.
pub(super) fn mixed_service_fields(probes: &[Probe]) -> Option<Finding> {
    let mut keys = BTreeMap::<&str, usize>::new();

    for probe in probes
        .iter()
        .filter(|probe| probe.signal != Signal::Metrics)
    {
        if let Some(field) = &probe.detected.service {
            *keys.entry(field.as_str()).or_default() += 1;
        }
    }

    if keys.len() < 2 {
        return None;
    }

    let mut keys = keys.into_iter().collect::<Vec<_>>();
    keys.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(right.0)));

    let told = keys
        .iter()
        .map(|(key, streams)| format!("{key} in {streams} stream(s)"))
        .collect::<Vec<_>>()
        .join(", ");

    Some(Finding::new(
        Severity::Warning,
        "mixed_service_fields",
        format!("The service is recorded under different keys: {told}."),
        "Pick one key for every signal (service.name, as OpenTelemetry traces have it) and \
         rename the others in their pipelines; until then, write the mapping into the knowledge \
         (MOLE.md) so investigations know it.",
    ))
}

pub(super) fn near_miss_findings(misses: &[NearMiss]) -> Vec<Finding> {
    misses
        .iter()
        .take(LISTED)
        .map(|miss| {
            let spellings = miss
                .spellings
                .iter()
                .map(|(name, signals)| {
                    let signals = signals
                        .iter()
                        .map(|signal| signal.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");

                    format!("{name} ({signals})")
                })
                .collect::<Vec<_>>()
                .join(", ");

            Finding::new(
                Severity::Warning,
                "service_name_near_miss",
                format!("One service seems to be written in several ways: {spellings}."),
                "Use one name at the sources, or map the others to it in the pipelines; record \
                 the other spellings as aliases in the service catalog so investigations find \
                 all of them.",
            )
        })
        .collect()
}

pub(super) fn one_signal_findings(only: &BTreeMap<Signal, Vec<String>>) -> Vec<Finding> {
    only.iter()
        .map(|(signal, names)| {
            Finding::new(
                Severity::Info,
                "service_in_one_signal",
                format!(
                    "Services seen only in {}: {}.",
                    signal.as_str(),
                    listed(names)
                ),
                "When these services write other signals too, those name them differently; \
                 otherwise nothing is missing.",
            )
        })
        .collect()
}

pub(super) fn environment_findings(groups: &[Vec<(String, u64)>]) -> Vec<Finding> {
    groups
        .iter()
        .take(LISTED)
        .map(|spellings| {
            let told = spellings
                .iter()
                .map(|(value, rows)| format!("{value} ({rows} rows)"))
                .collect::<Vec<_>>()
                .join(", ");

            let keep = spellings
                .first()
                .map(|(value, _)| value.as_str())
                .unwrap_or_default();

            let others = spellings
                .iter()
                .skip(1)
                .map(|(value, _)| format!("\"{value}\""))
                .collect::<Vec<_>>()
                .join(", ");

            Finding::new(
                Severity::Warning,
                "environment_spellings",
                format!("One environment is written in several ways: {told}."),
                format!(
                    "Map them to one value at intake (VRL: if includes([{others}], .env) {{ .env \
                     = \"{keep}\" }}, with the stream's environment field) and use that value at \
                     the sources."
                ),
            )
        })
        .collect()
}

pub(super) fn unit_findings(gaps: &BTreeMap<UnitGap, Vec<String>>) -> Vec<Finding> {
    gaps.iter()
        .map(|(gap, metrics)| {
            let (severity, kind, what, suggestion) = match gap {
                UnitGap::Duration => (
                    Severity::Info,
                    "metric_without_unit",
                    "measure a duration without saying its unit",
                    "Name durations with their unit, in seconds by convention (_seconds), so a \
                     reader knows whether 200 is milliseconds or seconds.",
                ),
                UnitGap::Size => (
                    Severity::Info,
                    "metric_without_unit",
                    "measure a size without saying its unit",
                    "Name sizes with their unit, in bytes by convention (_bytes).",
                ),
                UnitGap::CounterTotal => (
                    Severity::Info,
                    "counter_without_total",
                    "are counters whose names do not end in _total",
                    "Counters end in _total by the OpenMetrics convention: rename them at the \
                     exporter, or with a recording rule.",
                ),
            };

            Finding::new(
                severity,
                kind,
                format!("{} metric(s) {what}: {}.", metrics.len(), listed(metrics)),
                suggestion,
            )
        })
        .collect()
}

/// Some names, cut to [`LISTED`] with how many more there are.
pub(super) fn listed(names: &[String]) -> String {
    let mut text = names
        .iter()
        .take(LISTED)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");

    if names.len() > LISTED {
        text.push_str(&format!(" and {} more", names.len() - LISTED));
    }

    text
}

#[cfg(test)]
mod tests;
