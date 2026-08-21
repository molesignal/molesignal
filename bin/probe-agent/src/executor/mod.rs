// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod artifacts;
mod browser;
mod http;
mod network;
mod security;
mod ssh;

use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Result, anyhow};

use crate::protocol::v1::{
    self as wire, AssertionResult, ProbeAttempt, ProbeOutcome, ProbeResult, TimingBreakdown,
    probe_task,
};

const MAX_EXCERPT_BYTES: usize = 64 * 1024;
const MAX_METADATA_ENTRIES: usize = 64;
const MAX_STEP_METADATA_ENTRIES: usize = 16;

#[derive(Clone)]
pub(super) struct ExecutionContext {
    secrets: HashMap<String, Vec<u8>>,
    variables: HashMap<String, String>,
}

pub(super) struct AttemptOutcome {
    pub outcome: ProbeOutcome,
    pub assertions: Vec<AssertionResult>,
    pub response_excerpt: Vec<u8>,
    pub error_category: String,
    pub error_message: String,
    pub metadata: HashMap<String, String>,
    pub evidence: Vec<wire::StepEvidence>,
    pub artifacts: Vec<ArtifactPayload>,
}

pub(super) struct ArtifactPayload {
    pub kind: String,
    pub name: String,
    pub bytes: Vec<u8>,
}

impl ExecutionContext {
    fn new(task: &wire::ProbeTask) -> Self {
        Self {
            secrets: task
                .secrets
                .iter()
                .map(|secret| (secret.reference.clone(), secret.value.to_vec()))
                .collect(),
            variables: HashMap::new(),
        }
    }

    pub fn resolve(&self, value: &wire::Value) -> Result<String> {
        use wire::value::Source;
        match value.source.as_ref() {
            Some(Source::Literal(value)) => Ok(value.clone()),
            Some(Source::SecretReference(reference)) => String::from_utf8(
                self.secrets
                    .get(reference)
                    .ok_or_else(|| anyhow!("missing resolved Secret `{reference}`"))?
                    .clone(),
            )
            .map_err(|_| anyhow!("Secret `{reference}` is not UTF-8")),
            Some(Source::VariableReference(name)) => self
                .variables
                .get(name)
                .cloned()
                .ok_or_else(|| anyhow!("undefined Journey variable `{name}`")),
            None => Err(anyhow!("Value has no source")),
        }
    }

    pub fn insert_variable(&mut self, name: String, value: String) {
        self.variables.insert(name, value);
    }

    pub fn redact(&self, value: &str) -> String {
        self.secrets
            .values()
            .fold(value.to_string(), |text, bytes| {
                let Ok(secret) = std::str::from_utf8(bytes) else {
                    return text;
                };
                if secret.is_empty() {
                    return text;
                }
                let encoded =
                    url::form_urlencoded::byte_serialize(secret.as_bytes()).collect::<String>();
                let encoded_space = encoded.replace('+', "%20");
                let text = text.replace(secret, "[REDACTED]");
                let text = if encoded != secret {
                    text.replace(&encoded, "[REDACTED]")
                } else {
                    text
                };
                if encoded_space != encoded {
                    text.replace(&encoded_space, "[REDACTED]")
                } else {
                    text
                }
            })
    }
}

pub async fn execute(task: wire::ProbeTask, result_sequence: u64) -> Result<ProbeResult> {
    let started_at = now_micros();
    let mut attempts = Vec::new();
    let mut final_assertions = Vec::new();
    let mut final_outcome = ProbeOutcome::Unknown;
    let mut captured_artifacts = Vec::new();
    let max_attempts = task.max_attempts.clamp(1, 3);
    for number in 1..=max_attempts {
        let attempt_started = now_micros();
        let mut context = ExecutionContext::new(&task);
        let executed = run_once(&task, &mut context).await;
        let attempt_finished = now_micros();
        let mut outcome = match executed {
            Ok(outcome) => outcome,
            Err(error) => AttemptOutcome {
                outcome: ProbeOutcome::Unknown,
                assertions: Vec::new(),
                response_excerpt: Vec::new(),
                error_category: "executor_error".to_string(),
                error_message: context.redact(&error.to_string()),
                metadata: HashMap::new(),
                evidence: Vec::new(),
                artifacts: Vec::new(),
            },
        };
        normalize_attempt(&mut outcome);
        final_outcome = outcome.outcome;
        final_assertions = outcome.assertions.clone();
        captured_artifacts.extend(outcome.artifacts);
        attempts.push(ProbeAttempt {
            number,
            started_at_micros: attempt_started,
            finished_at_micros: attempt_finished,
            outcome: outcome.outcome as i32,
            timing: Some(TimingBreakdown {
                total_micros: Some((attempt_finished - attempt_started).max(0) as u64),
                ..Default::default()
            }),
            error_category: outcome.error_category,
            error_message: outcome.error_message,
            bounded_response_excerpt: outcome.response_excerpt.into(),
            metadata: outcome.metadata,
            evidence: outcome.evidence,
        });
        if matches!(
            outcome.outcome,
            ProbeOutcome::Healthy | ProbeOutcome::Flaky | ProbeOutcome::Degraded
        ) {
            break;
        }
    }
    let flaky = recovered_after_retry(&attempts, final_outcome);
    if flaky {
        final_outcome = ProbeOutcome::Flaky;
    }
    let finished_at = now_micros();
    let upload = artifacts::upload(&task, captured_artifacts).await;
    let mut metadata = HashMap::new();
    if upload.captured_count > 0 {
        metadata.insert(
            "artifact_capture_count".into(),
            upload.captured_count.to_string(),
        );
        metadata.insert(
            "artifact_upload_count".into(),
            upload.receipts.len().to_string(),
        );
    }
    if let Some(error) = upload.error {
        metadata.insert("artifact_upload_error".into(), error);
    }
    if flaky {
        metadata.insert("flaky".into(), "true".into());
        metadata.insert(
            "flaky_recovered_on_attempt".into(),
            attempts.len().to_string(),
        );
    }
    Ok(ProbeResult {
        task_id: task.task_id,
        lease_token: task.lease_token,
        result_sequence,
        started_at_micros: started_at,
        finished_at_micros: finished_at,
        outcome: final_outcome as i32,
        attempts,
        assertions: final_assertions,
        artifacts: upload.receipts,
        metadata,
        secret_versions: task
            .secrets
            .into_iter()
            .map(|secret| (secret.reference, secret.version))
            .collect(),
    })
}

async fn run_once(
    task: &wire::ProbeTask,
    context: &mut ExecutionContext,
) -> Result<AttemptOutcome> {
    let spec = task
        .spec
        .as_ref()
        .ok_or_else(|| anyhow!("Probe task has no Spec"))?;
    match spec {
        probe_task::Spec::Http(spec) => http::execute(task, spec, context).await,
        probe_task::Spec::Tcp(spec) => network::execute_tcp(task, spec, context).await,
        probe_task::Spec::Ssh(spec) => ssh::execute(task, spec, context).await,
        probe_task::Spec::Dns(spec) => network::execute_dns(task, spec).await,
        probe_task::Spec::Tls(spec) => network::execute_tls(task, spec).await,
        probe_task::Spec::Icmp(spec) => network::execute_icmp(task, spec).await,
        probe_task::Spec::Grpc(spec) => network::execute_grpc(task, spec).await,
        probe_task::Spec::Browser(spec) => browser::execute(task, spec, context).await,
    }
}

pub(super) fn unknown(category: &str, message: impl Into<String>) -> AttemptOutcome {
    AttemptOutcome {
        outcome: ProbeOutcome::Unknown,
        assertions: Vec::new(),
        response_excerpt: Vec::new(),
        error_category: category.to_string(),
        error_message: message.into(),
        metadata: HashMap::new(),
        evidence: Vec::new(),
        artifacts: Vec::new(),
    }
}

pub(super) fn now_micros() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros()
        .min(i64::MAX as u128) as i64
}

fn normalize_attempt(outcome: &mut AttemptOutcome) {
    outcome.error_category = truncate_string(std::mem::take(&mut outcome.error_category), 128);
    outcome.error_message = truncate_string(std::mem::take(&mut outcome.error_message), 4096);
    outcome.response_excerpt.truncate(MAX_EXCERPT_BYTES);
    outcome.metadata = bound_metadata(std::mem::take(&mut outcome.metadata), MAX_METADATA_ENTRIES);
    for assertion in &mut outcome.assertions {
        assertion.actual = truncate_string(std::mem::take(&mut assertion.actual), 4096);
        assertion.message = truncate_string(std::mem::take(&mut assertion.message), 4096);
    }
    outcome.evidence.truncate(100);
    for evidence in &mut outcome.evidence {
        evidence.step_id = truncate_string(std::mem::take(&mut evidence.step_id), 128);
        evidence.name = truncate_string(std::mem::take(&mut evidence.name), 256);
        evidence.action = truncate_string(std::mem::take(&mut evidence.action), 64);
        evidence.error_category =
            truncate_string(std::mem::take(&mut evidence.error_category), 128);
        evidence.error_message = truncate_string(std::mem::take(&mut evidence.error_message), 4096);
        evidence.metadata = bound_metadata(
            std::mem::take(&mut evidence.metadata),
            MAX_STEP_METADATA_ENTRIES,
        );
    }
}

fn recovered_after_retry(attempts: &[ProbeAttempt], final_outcome: ProbeOutcome) -> bool {
    final_outcome == ProbeOutcome::Healthy
        && attempts.len() > 1
        && attempts[..attempts.len() - 1].iter().any(|attempt| {
            matches!(
                ProbeOutcome::try_from(attempt.outcome),
                Ok(ProbeOutcome::Failing | ProbeOutcome::Unknown)
            )
        })
}

fn bound_metadata(
    metadata: HashMap<String, String>,
    max_entries: usize,
) -> HashMap<String, String> {
    metadata
        .into_iter()
        .filter(|(key, _)| !key.is_empty())
        .take(max_entries)
        .map(|(key, value)| (truncate_string(key, 128), truncate_string(value, 4096)))
        .collect()
}

fn truncate_string(mut value: String, max: usize) -> String {
    if value.len() <= max {
        return value;
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    value
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{ExecutionContext, recovered_after_retry};
    use crate::protocol::v1::{ProbeAttempt, ProbeOutcome};

    #[test]
    fn redacts_plain_and_url_encoded_secrets() {
        let context = ExecutionContext {
            secrets: HashMap::from([("secret".into(), b"a b/c".to_vec())]),
            variables: HashMap::new(),
        };

        let redacted = context.redact("plain=a b/c query=a+b%2Fc alternate=a%20b%2Fc");
        assert_eq!(
            redacted,
            "plain=[REDACTED] query=[REDACTED] alternate=[REDACTED]"
        );
    }

    #[test]
    fn recovered_retry_is_flaky() {
        let attempts = [
            ProbeAttempt {
                outcome: ProbeOutcome::Failing as i32,
                ..Default::default()
            },
            ProbeAttempt {
                outcome: ProbeOutcome::Healthy as i32,
                ..Default::default()
            },
        ];

        assert!(recovered_after_retry(&attempts, ProbeOutcome::Healthy));
        assert!(!recovered_after_retry(&attempts, ProbeOutcome::Failing));
        assert!(!recovered_after_retry(
            &attempts[1..],
            ProbeOutcome::Healthy
        ));
    }
}
