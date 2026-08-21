// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::{BTreeMap, HashSet};

use crate::{
    app::synthetics::{artifact_expiry, artifact_object_key, artifact_targets},
    domain::synthetics::{
        AssertionObservation, AssertionSeverity, ProbeAgent, ProbeAttempt, ProbeOutcome,
        ProbeStepEvidence, ProbeTask, SyntheticResult, SyntheticResultArtifact, TimingBreakdown,
    },
    protocol::probe::v1 as wire,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const MAX_ATTEMPTS: usize = 3;
const MAX_EXCERPT_BYTES: usize = 64 * 1024;
const MAX_METADATA_ENTRIES: usize = 64;
const MAX_STEP_EVIDENCE: usize = 100;
const MAX_STEP_METADATA_ENTRIES: usize = 16;
const MAX_ARTIFACTS: usize = 30;
const MAX_ARTIFACT_BYTES: u64 = 75 * 1024 * 1024;

pub(crate) fn result_from_wire(
    agent: &ProbeAgent,
    task: &ProbeTask,
    value: wire::ProbeResult,
) -> Result<SyntheticResult> {
    validate_result(task, &value)?;
    let received_at = TimestampMicros::now();
    let targets = artifact_targets(task, &value.lease_token);
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
        received_at,
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
        artifacts: value
            .artifacts
            .into_iter()
            .map(|artifact| artifact_from_wire(task, &targets, artifact, received_at))
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
    if value.attempts.iter().any(|attempt| {
        attempt.started_at_micros < value.started_at_micros
            || attempt.finished_at_micros > value.finished_at_micros
    }) {
        return Err(Error::invalid(
            "Probe Attempt falls outside its result window",
        ));
    }
    if value.assertions.len() > 256 {
        return Err(Error::invalid("Probe result contains too many Assertions"));
    }
    if !metadata_is_bounded(&value.metadata, MAX_METADATA_ENTRIES)
        || value.secret_versions.len() > 64
        || value
            .secret_versions
            .keys()
            .any(|key| key.is_empty() || key.len() > 256)
    {
        return Err(Error::invalid("Probe result metadata is too large"));
    }
    if !value.artifacts.is_empty() {
        let total_bytes = value
            .artifacts
            .iter()
            .try_fold(0_u64, |total, artifact| {
                total.checked_add(artifact.content_length)
            })
            .ok_or_else(|| Error::invalid("Probe Artifact size overflow"))?;
        if value.artifacts.len() > MAX_ARTIFACTS || total_bytes > MAX_ARTIFACT_BYTES {
            return Err(Error::invalid("Probe result contains too many Artifacts"));
        }
        let mut artifact_ids = HashSet::with_capacity(value.artifacts.len());
        for artifact in &value.artifacts {
            if artifact.artifact_id.is_empty()
                || artifact.artifact_id.len() > 128
                || artifact.kind.is_empty()
                || artifact.kind.len() > 32
                || artifact.name.is_empty()
                || artifact.name.len() > 128
                || artifact.sha256.len() != 32
                || artifact.content_length == 0
                || artifact.content_length > 10 * 1024 * 1024
                || !artifact.uploaded
                || !artifact_ids.insert(artifact.artifact_id.as_str())
            {
                return Err(Error::invalid("invalid Probe Artifact receipt"));
            }
        }
    }
    Ok(())
}

fn artifact_from_wire(
    task: &ProbeTask,
    targets: &[crate::app::synthetics::SyntheticArtifactTarget],
    value: wire::Artifact,
    received_at: TimestampMicros,
) -> Result<SyntheticResultArtifact> {
    let target = targets
        .iter()
        .find(|target| target.id == value.artifact_id)
        .ok_or_else(|| Error::unauthorized("Probe Artifact was not issued for this task lease"))?;
    if value.kind != target.kind
        || value.name != target.name
        || value.content_length > target.max_bytes
    {
        return Err(Error::invalid(
            "Probe Artifact receipt does not match its upload target",
        ));
    }
    Ok(SyntheticResultArtifact {
        id: Id::from_string(value.artifact_id),
        name: value.name,
        kind: value.kind,
        object_key: artifact_object_key(task, &target.id),
        content_type: target.content_type.clone(),
        content_length: value.content_length,
        sha256: hex::encode(value.sha256),
        expires_at: artifact_expiry(received_at),
        created_at: received_at,
    })
}

fn attempt_from_wire(value: wire::ProbeAttempt) -> Result<ProbeAttempt> {
    if value.number == 0
        || value.started_at_micros <= 0
        || value.finished_at_micros < value.started_at_micros
        || value.bounded_response_excerpt.len() > MAX_EXCERPT_BYTES
        || value.error_category.len() > 128
        || value.error_message.len() > 4096
        || !metadata_is_bounded(&value.metadata, MAX_METADATA_ENTRIES)
        || value.evidence.len() > MAX_STEP_EVIDENCE
        || value.evidence.iter().any(|evidence| {
            evidence.started_at_micros < value.started_at_micros
                || evidence.finished_at_micros > value.finished_at_micros
        })
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
        evidence: value
            .evidence
            .into_iter()
            .map(step_evidence_from_wire)
            .collect::<Result<_>>()?,
    })
}

fn step_evidence_from_wire(value: wire::StepEvidence) -> Result<ProbeStepEvidence> {
    if value.step_id.is_empty()
        || value.step_id.len() > 128
        || value.name.len() > 256
        || value.action.is_empty()
        || value.action.len() > 64
        || value.started_at_micros <= 0
        || value.finished_at_micros < value.started_at_micros
        || value.error_category.len() > 128
        || value.error_message.len() > 4096
        || !metadata_is_bounded(&value.metadata, MAX_STEP_METADATA_ENTRIES)
    {
        return Err(Error::invalid("invalid Probe Step Evidence"));
    }
    Ok(ProbeStepEvidence {
        step_id: Id::from_string(value.step_id),
        name: value.name,
        action: value.action,
        started_at: TimestampMicros(value.started_at_micros),
        finished_at: TimestampMicros(value.finished_at_micros),
        outcome: outcome_from_wire(value.outcome)?,
        error_category: non_empty(value.error_category),
        error_message: non_empty(value.error_message),
        metadata: value.metadata.into_iter().collect(),
    })
}

fn assertion_from_wire(value: wire::AssertionResult) -> Result<AssertionObservation> {
    if value.assertion_id.is_empty()
        || value.assertion_id.len() > 128
        || value.actual.len() > 4096
        || value.message.len() > 4096
    {
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
        Some(wire::ProbeOutcome::Flaky) => Ok(ProbeOutcome::Flaky),
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

fn metadata_is_bounded(
    metadata: &std::collections::HashMap<String, String>,
    max_entries: usize,
) -> bool {
    metadata.len() <= max_entries
        && metadata
            .iter()
            .all(|(key, value)| !key.is_empty() && key.len() <= 128 && value.len() <= 4096)
}
