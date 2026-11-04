// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Every scheduled run of a check, also as a row of the organization's `_synthetic_results` log
//! stream, where time ranges, availability, dashboards and alert rules read runs with SQL.
//! Postgres keeps the full result.
//!
//! A run is queued when its result is recorded and written in batches by a background worker:
//! writing goes through intake, which the probe's report should not wait on. When the queue is
//! full the row is dropped, and the run stays in Postgres all the same.

use serde_json::{Map, Value, json};
use tokio::sync::mpsc;

use crate::domain::synthetics::{ProbeOutcome, SyntheticMonitor, SyntheticResult};

/// The internal stream the runs go to.
pub const RESULT_STREAM: &str = "_synthetic_results";

/// Runs waiting to be written, at most.
const QUEUE_CAPACITY: usize = 4096;

#[derive(Clone)]
pub struct ResultStreamQueue {
    sender: mpsc::Sender<SyntheticResult>,
}

pub type ResultStreamReceiver = mpsc::Receiver<SyntheticResult>;

/// Both ends of the queue: the sender for the synthetics service, the receiver for the writer.
pub fn result_stream_queue() -> (ResultStreamQueue, ResultStreamReceiver) {
    let (sender, receiver) = mpsc::channel(QUEUE_CAPACITY);
    (ResultStreamQueue { sender }, receiver)
}

impl ResultStreamQueue {
    pub fn push(&self, result: SyntheticResult) {
        if let Err(error) = self.sender.try_send(result) {
            tracing::debug!(%error, "synthetic result stream row dropped");
        }
    }
}

/// Whether a run counts toward availability: one that passed, flapped or slowed was available,
/// one that failed was not, and one that could not be judged or never ran says nothing.
pub fn available(outcome: ProbeOutcome) -> Option<bool> {
    match outcome {
        ProbeOutcome::Healthy | ProbeOutcome::Flaky | ProbeOutcome::Degraded => Some(true),
        ProbeOutcome::Failing => Some(false),
        ProbeOutcome::Unknown | ProbeOutcome::Skipped => None,
    }
}

/// The row of a run. Field names carry no dots, which SQL reads as qualifiers; a value a run does
/// not have is left out rather than written as null.
pub fn stream_fields(
    result: &SyntheticResult,
    monitor: Option<&SyntheticMonitor>,
    location_name: Option<&str>,
) -> Map<String, Value> {
    let mut fields = Map::new();
    let attempt = result.attempts.last();
    let millis = |micros: Option<u64>| micros.map(|micros| json!(micros as f64 / 1_000.0));

    fields.insert("result_id".into(), json!(result.id.0));
    fields.insert("check_id".into(), json!(result.monitor_id.0));
    fields.insert("revision_id".into(), json!(result.monitor_revision_id.0));
    fields.insert("location_id".into(), json!(result.location_id.0));
    fields.insert("outcome".into(), json!(result.outcome.as_str()));
    fields.insert("attempts".into(), json!(result.attempts.len()));

    // The step that answered last says the status a run ended on.
    let status_code = attempt.and_then(|attempt| {
        attempt
            .evidence
            .iter()
            .rev()
            .find_map(|step| step.metadata.get("status"))
            .and_then(|status| status.parse::<u16>().ok())
    });

    let optional = [
        ("check_name", monitor.map(|monitor| json!(monitor.name))),
        (
            "check_kind",
            monitor.map(|monitor| json!(monitor.kind.as_str())),
        ),
        ("location_name", location_name.map(|name| json!(name))),
        ("agent_id", result.agent_id.as_ref().map(|id| json!(id.0))),
        (
            "available",
            available(result.outcome).map(|value| json!(value)),
        ),
        (
            "duration_ms",
            millis(attempt.and_then(|attempt| attempt.timing.total_time)),
        ),
        (
            "dns_ms",
            millis(attempt.and_then(|attempt| attempt.timing.dns_time)),
        ),
        (
            "connect_ms",
            millis(attempt.and_then(|attempt| attempt.timing.connect_time)),
        ),
        (
            "tls_ms",
            millis(attempt.and_then(|attempt| attempt.timing.tls_time)),
        ),
        (
            "first_byte_ms",
            millis(attempt.and_then(|attempt| attempt.timing.first_byte_time)),
        ),
        ("status_code", status_code.map(|status| json!(status))),
        (
            "error_category",
            attempt
                .and_then(|attempt| attempt.error_category.as_ref())
                .map(|category| json!(category)),
        ),
        (
            "skip_reason",
            result
                .metadata
                .get("skip_reason")
                .map(|reason| json!(reason)),
        ),
        (
            "trace_id",
            result.metadata.get("trace_id").map(|id| json!(id)),
        ),
        (
            "team_id",
            monitor
                .and_then(|monitor| monitor.team_id.as_ref())
                .map(|id| json!(id.0)),
        ),
        (
            "tags",
            monitor
                .filter(|monitor| !monitor.tags.is_empty())
                .map(|monitor| json!(monitor.tags.join(","))),
        ),
    ];

    for (name, value) in optional {
        if let Some(value) = value {
            fields.insert(name.into(), value);
        }
    }

    fields
}

#[cfg(test)]
mod tests;
