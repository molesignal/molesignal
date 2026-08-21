// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use sha2::{Digest as _, Sha256};

use crate::{
    domain::synthetics::{BrowserAction, MonitorSpec, ProbeTask},
    shared::time::TimestampMicros,
};

pub const MAX_SCREENSHOT_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_HAR_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_TRACE_BYTES: u64 = 5 * 1024 * 1024;
pub const DEFAULT_ARTIFACT_RETENTION_DAYS: i64 = 7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntheticArtifactTarget {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub content_type: String,
    pub max_bytes: u64,
    pub expires_at: TimestampMicros,
}

pub fn artifact_targets(task: &ProbeTask, lease_token: &str) -> Vec<SyntheticArtifactTarget> {
    let MonitorSpec::Browser(spec) = &task.spec else {
        return Vec::new();
    };
    let mut definitions = spec
        .steps
        .iter()
        .filter_map(|step| match &step.action {
            BrowserAction::Screenshot { name, .. } => Some((
                "screenshot",
                if name.trim().is_empty() {
                    "capture"
                } else {
                    name.trim()
                },
                "image/png",
                MAX_SCREENSHOT_BYTES,
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    if spec.capture_screenshot_on_failure {
        definitions.push(("screenshot", "failure", "image/png", MAX_SCREENSHOT_BYTES));
    }
    if spec.capture_har_on_failure {
        definitions.push(("har", "failure", "application/json", MAX_HAR_BYTES));
    }
    if spec.capture_trace_on_failure {
        definitions.push(("trace", "failure", "application/json", MAX_TRACE_BYTES));
    }
    (0..task.max_attempts.clamp(1, 3))
        .flat_map(|_| definitions.iter().copied())
        .enumerate()
        .map(
            |(index, (kind, name, content_type, max_bytes))| SyntheticArtifactTarget {
                id: target_id(task, lease_token, kind, name, index),
                name: name.to_string(),
                kind: kind.to_string(),
                content_type: content_type.to_string(),
                max_bytes,
                expires_at: task.deadline_at,
            },
        )
        .collect()
}

pub fn artifact_object_key(task: &ProbeTask, artifact_id: &str) -> String {
    format!(
        "{}/synthetics/artifacts/{}/{}",
        task.organization_id, task.id, artifact_id
    )
}

pub fn artifact_expiry(created_at: TimestampMicros) -> TimestampMicros {
    TimestampMicros(
        created_at.0.saturating_add(
            DEFAULT_ARTIFACT_RETENTION_DAYS
                .saturating_mul(24 * 60 * 60)
                .saturating_mul(1_000_000),
        ),
    )
}

fn target_id(task: &ProbeTask, lease_token: &str, kind: &str, name: &str, index: usize) -> String {
    let mut digest = Sha256::new();
    digest.update(lease_token.as_bytes());
    digest.update(b"\0molesignal-synthetic-artifact\0");
    digest.update(task.id.as_str().as_bytes());
    digest.update(b"\0");
    digest.update(kind.as_bytes());
    digest.update(b"\0");
    digest.update(name.as_bytes());
    digest.update(b"\0");
    digest.update(index.to_le_bytes());
    hex::encode(digest.finalize())
}

#[cfg(test)]
mod tests {
    use super::target_id;
    use crate::{
        domain::synthetics::{MonitorSpec, ProbeTask, ProbeTaskState},
        shared::{ids::Id, time::TimestampMicros},
    };

    fn task() -> ProbeTask {
        ProbeTask {
            id: Id::from_string("task"),
            organization_id: Id::from_string("org"),
            monitor_id: Id::from_string("monitor"),
            monitor_revision_id: Id::from_string("revision"),
            location_id: Id::from_string("location"),
            spec: MonitorSpec::Heartbeat,
            is_test: false,
            state: ProbeTaskState::Running,
            scheduled_at: TimestampMicros(1),
            deadline_at: TimestampMicros(2),
            timeout_millis: 1_000,
            max_attempts: 1,
            lease_agent_id: None,
            lease_token_hash: None,
            leased_until: None,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        }
    }

    #[test]
    fn target_ids_are_lease_bound_and_stable() {
        let task = task();
        let first = target_id(&task, "lease-a", "screenshot", "failure", 0);
        assert_eq!(
            first,
            target_id(&task, "lease-a", "screenshot", "failure", 0)
        );
        assert_ne!(
            first,
            target_id(&task, "lease-b", "screenshot", "failure", 0)
        );
        assert_ne!(first, target_id(&task, "lease-a", "har", "failure", 0));
    }
}
