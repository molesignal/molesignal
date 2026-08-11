// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use crate::{
    domain::synthetics::{
        AssertionObservation, AssertionSeverity, ProbeAgent, ProbeAttempt, ProbeOutcome, ProbeTask,
        SyntheticResult, TimingBreakdown,
    },
    protocol::probe::v1 as wire,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const MAX_ATTEMPTS: usize = 3;
const MAX_EXCERPT_BYTES: usize = 64 * 1024;
const MAX_METADATA_ENTRIES: usize = 64;

pub(crate) fn result_from_wire(
    agent: &ProbeAgent,
    task: &ProbeTask,
    value: wire::ProbeResult,
) -> Result<SyntheticResult> {
    validate_result(task, &value)?;
    Ok(SyntheticResult {
        id: Id::new(),
        organization_id: task.organization_id.clone(),
        monitor_id: task.monitor_id.clone(),
        monitor_revision_id: task.monitor_revision_id.clone(),
        location_id: task.location_id.clone(),
        agent_id: Some(agent.id.clone()),
        task_id: task.id.clone(),
        is_test: task.is_test,
        result_sequence: Some(value.result_sequence),
        scheduled_at: task.scheduled_at,
        started_at: TimestampMicros(value.started_at_micros),
        finished_at: TimestampMicros(value.finished_at_micros),
        received_at: TimestampMicros::now(),
        outcome: outcome_from_wire(value.outcome)?,
        attempts: value
            .attempts
            .into_iter()
            .map(attempt_from_wire)
            .collect::<Result<_>>()?,
        assertions: value
            .assertions
            .into_iter()
            .map(assertion_from_wire)
            .collect::<Result<_>>()?,
        secret_versions: value.secret_versions.into_iter().collect(),
        protocol_version: agent.protocol_version,
        metadata: value.metadata.into_iter().collect(),
    })
}

fn validate_result(task: &ProbeTask, value: &wire::ProbeResult) -> Result<()> {
    if value.task_id != task.id.as_str() {
        return Err(Error::unauthorized(
            "Probe result task does not match its lease",
        ));
    }
    if value.result_sequence == 0 {
        return Err(Error::invalid("Probe result sequence must be positive"));
    }
    if value.started_at_micros <= 0 || value.finished_at_micros < value.started_at_micros {
        return Err(Error::invalid("invalid Probe result time window"));
    }
    if value.attempts.is_empty() || value.attempts.len() > MAX_ATTEMPTS {
        return Err(Error::invalid("Probe result must contain 1 to 3 Attempts"));
    }
    if value.metadata.len() > MAX_METADATA_ENTRIES || value.secret_versions.len() > 64 {
        return Err(Error::invalid("Probe result metadata is too large"));
    }
    if !value.artifacts.is_empty() {
        for artifact in &value.artifacts {
            if artifact.artifact_id.is_empty()
                || artifact.sha256.len() != 32
                || artifact.content_length > 50 * 1024 * 1024
                || !artifact.uploaded
            {
                return Err(Error::invalid("invalid Probe Artifact receipt"));
            }
        }
    }
    Ok(())
}

fn attempt_from_wire(value: wire::ProbeAttempt) -> Result<ProbeAttempt> {
    if value.number == 0
        || value.started_at_micros <= 0
        || value.finished_at_micros < value.started_at_micros
        || value.bounded_response_excerpt.len() > MAX_EXCERPT_BYTES
        || value.error_category.len() > 128
        || value.error_message.len() > 4096
        || value.metadata.len() > MAX_METADATA_ENTRIES
    {
        return Err(Error::invalid("invalid Probe Attempt"));
    }
    let timing = value.timing.unwrap_or_default();
    Ok(ProbeAttempt {
        number: value.number,
        started_at: TimestampMicros(value.started_at_micros),
        finished_at: TimestampMicros(value.finished_at_micros),
        outcome: outcome_from_wire(value.outcome)?,
        timing: TimingBreakdown {
            dns_micros: timing.dns_micros,
            connect_micros: timing.connect_micros,
            tls_micros: timing.tls_micros,
            first_byte_micros: timing.first_byte_micros,
            total_micros: timing.total_micros,
        },
        error_category: non_empty(value.error_category),
        error_message: non_empty(value.error_message),
        bounded_response_excerpt: (!value.bounded_response_excerpt.is_empty())
            .then(|| value.bounded_response_excerpt.to_vec()),
        metadata: value.metadata.into_iter().collect::<BTreeMap<_, _>>(),
    })
}

fn assertion_from_wire(value: wire::AssertionResult) -> Result<AssertionObservation> {
    if value.assertion_id.is_empty() || value.actual.len() > 4096 || value.message.len() > 4096 {
        return Err(Error::invalid("invalid Probe Assertion result"));
    }
    let severity = match wire::AssertionSeverity::try_from(value.severity).ok() {
        Some(wire::AssertionSeverity::Warning) => AssertionSeverity::Warning,
        Some(wire::AssertionSeverity::Critical) => AssertionSeverity::Critical,
        _ => return Err(Error::invalid("invalid Probe Assertion severity")),
    };
    Ok(AssertionObservation {
        assertion_id: Id::from_string(value.assertion_id),
        severity,
        passed: value.passed,
        actual: non_empty(value.actual),
        message: non_empty(value.message),
    })
}

fn outcome_from_wire(value: i32) -> Result<ProbeOutcome> {
    match wire::ProbeOutcome::try_from(value).ok() {
        Some(wire::ProbeOutcome::Healthy) => Ok(ProbeOutcome::Healthy),
        Some(wire::ProbeOutcome::Degraded) => Ok(ProbeOutcome::Degraded),
        Some(wire::ProbeOutcome::Failing) => Ok(ProbeOutcome::Failing),
        Some(wire::ProbeOutcome::Unknown) => Ok(ProbeOutcome::Unknown),
        Some(wire::ProbeOutcome::Skipped) => Ok(ProbeOutcome::Skipped),
        _ => Err(Error::invalid("invalid Probe outcome")),
    }
}

fn non_empty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}
