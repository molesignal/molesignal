// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod http;
mod network;
mod security;

use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Result, anyhow};

use crate::protocol::v1::{
    self as wire, AssertionResult, ProbeAttempt, ProbeOutcome, ProbeResult, TimingBreakdown,
    probe_task,
};

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
            .fold(value.to_string(), |text, secret| {
                String::from_utf8(secret.clone())
                    .ok()
                    .filter(|secret| !secret.is_empty())
                    .map_or(text.clone(), |secret| text.replace(&secret, "[REDACTED]"))
            })
    }
}

pub async fn execute(task: wire::ProbeTask, result_sequence: u64) -> Result<ProbeResult> {
    let started_at = now_micros();
    let mut attempts = Vec::new();
    let mut final_assertions = Vec::new();
    let mut final_outcome = ProbeOutcome::Unknown;
    let max_attempts = task.max_attempts.clamp(1, 3);
    for number in 1..=max_attempts {
        let attempt_started = now_micros();
        let mut context = ExecutionContext::new(&task);
        let executed = run_once(&task, &mut context).await;
        let attempt_finished = now_micros();
        let outcome = match executed {
            Ok(outcome) => outcome,
            Err(error) => AttemptOutcome {
                outcome: ProbeOutcome::Unknown,
                assertions: Vec::new(),
                response_excerpt: Vec::new(),
                error_category: "executor_error".to_string(),
                error_message: context.redact(&error.to_string()),
                metadata: HashMap::new(),
            },
        };
        final_outcome = outcome.outcome;
        final_assertions = outcome.assertions.clone();
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
        });
        if matches!(
            outcome.outcome,
            ProbeOutcome::Healthy | ProbeOutcome::Degraded
        ) {
            break;
        }
    }
    let finished_at = now_micros();
    Ok(ProbeResult {
        task_id: task.task_id,
        lease_token: task.lease_token,
        result_sequence,
        started_at_micros: started_at,
        finished_at_micros: finished_at,
        outcome: final_outcome as i32,
        attempts,
        assertions: final_assertions,
        artifacts: Vec::new(),
        metadata: HashMap::new(),
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
        probe_task::Spec::Dns(spec) => network::execute_dns(task, spec).await,
        probe_task::Spec::Tls(spec) => network::execute_tls(task, spec).await,
        probe_task::Spec::Icmp(spec) => network::execute_icmp(task, spec).await,
        probe_task::Spec::Grpc(spec) => network::execute_grpc(task, spec).await,
        probe_task::Spec::Browser(spec) => http::execute_browser(task, spec, context).await,
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
    }
}

pub(super) fn now_micros() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros()
        .min(i64::MAX as u128) as i64
}
