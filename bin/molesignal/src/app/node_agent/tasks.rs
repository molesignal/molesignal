// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Node tasks: creation under the quota, cancellation, reading, and the expiry sweep.

use domain::node_agent::{NodeTask, TaskEvent, TaskFilter, TaskKind, TaskState, Transition};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Actor, NodeAgentService};

/// Most expired tasks one sweep ends.
const SWEEP_LIMIT: u32 = 500;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateTaskInput {
    pub node_id: Id,
    pub kind: TaskKind,
    pub spec: Value,
    /// The task never runs past this many seconds from now.
    #[serde(default = "default_timeout_secs")]
    pub timeout_seconds: u32,
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
}

const fn default_timeout_secs() -> u32 {
    3600
}

const fn default_max_attempts() -> u32 {
    3
}

impl NodeAgentService {
    pub async fn create_task(
        &self,
        org_id: &Id,
        actor: &Actor,
        input: CreateTaskInput,
    ) -> Result<NodeTask> {
        if !(10..=86_400).contains(&input.timeout_seconds) {
            return Err(Error::invalid(
                "`timeoutSeconds` must be between 10 and 86400",
            ));
        }

        if !(1..=10).contains(&input.max_attempts) {
            return Err(Error::invalid("`maxAttempts` must be between 1 and 10"));
        }

        NodeTask::validate_spec(input.kind, &input.spec)?;
        let node = self.repository.get_node(org_id, &input.node_id).await?;

        if !node.capabilities.supports(input.kind) {
            return Err(Error::invalid(format!(
                "the node does not declare `{}` tasks",
                input.kind.as_str()
            )));
        }

        let now = TimestampMicros::now();

        // Tasks whose lease or deadline passed give their slots back before the quota counts.
        self.repository
            .expire_due_tasks(Some(org_id), now, SWEEP_LIMIT)
            .await?;

        let quota = self.repository.task_quota(org_id).await?;

        let task = NodeTask {
            id: Id::new_uuid(),
            org_id: org_id.clone(),
            node_id: input.node_id,
            kind: input.kind,
            spec: input.spec,
            state: TaskState::Queued,
            attempt: 1,
            max_attempts: input.max_attempts,
            available_at: now,
            lease_expires_at: None,
            deadline: TimestampMicros(now.0 + i64::from(input.timeout_seconds) * 1_000_000),
            cancel_requested_at: None,
            cancel_requested_by: None,
            result: None,
            failure: None,
            created_by: actor.id.clone(),
            created_at: now,
            updated_at: now,
            finished_at: None,
        };

        let task = self.repository.create_task(&task, &quota).await?;

        self.audit
            .record(
                org_id,
                actor,
                "node_agent.task.create",
                ("node_agent_task", task.id.as_str()),
                json!({
                    "nodeId": task.node_id,
                    "kind": task.kind,
                    "tool": task.spec.get("tool"),
                    "deadline": task.deadline,
                    "maxAttempts": task.max_attempts,
                }),
            )
            .await;

        Ok(task)
    }

    /// Cancels a task: a queued one ends now; one on its node ends with the node's result or its
    /// lease.
    pub async fn cancel_task(&self, org_id: &Id, actor: &Actor, task_id: &Id) -> Result<NodeTask> {
        let event = TaskEvent::Cancel {
            by: actor.id.clone(),
        };

        let (task, transition) = self
            .repository
            .apply_task_event(org_id, None, task_id, event, TimestampMicros::now())
            .await?
            .ok_or_else(|| Error::not_found(format!("task `{}` not found", task_id.as_str())))?;

        if transition.changed() {
            let outcome = match transition {
                Transition::CancelRequested => "requested",
                _ => "cancelled",
            };

            self.audit
                .record(
                    org_id,
                    actor,
                    "node_agent.task.cancel",
                    ("node_agent_task", task.id.as_str()),
                    json!({"nodeId": task.node_id, "outcome": outcome, "state": task.state}),
                )
                .await;
        }

        Ok(task)
    }

    pub async fn get_task(&self, org_id: &Id, task_id: &Id) -> Result<NodeTask> {
        self.repository
            .expire_due_tasks(Some(org_id), TimestampMicros::now(), SWEEP_LIMIT)
            .await?;

        self.repository.get_task(org_id, task_id).await
    }

    pub async fn list_tasks(&self, org_id: &Id, filter: TaskFilter) -> Result<Vec<NodeTask>> {
        self.repository
            .expire_due_tasks(Some(org_id), TimestampMicros::now(), SWEEP_LIMIT)
            .await?;

        self.repository.list_tasks(org_id, &filter).await
    }

    /// Ends tasks whose deadline or lease passed, in every organization. Returns how many changed.
    pub async fn sweep_expired(&self, now: TimestampMicros) -> Result<usize> {
        let changed = self
            .repository
            .expire_due_tasks(None, now, SWEEP_LIMIT)
            .await?;

        Ok(changed.len())
    }
}
