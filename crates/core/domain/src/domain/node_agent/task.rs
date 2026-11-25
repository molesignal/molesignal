// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Node tasks and their leases (control protocol §7), as pure transitions.
//!
//! A task is offered to its node as the lease `(task_id, attempt)`. The node accepts or rejects it,
//! heartbeats renew it (never shorter, never past the deadline), and the first result of the
//! current attempt is recorded; a repeated result is a duplicate and changes nothing. An offer that
//! is never accepted, or that the node turns down for quota or draining, is offered again as the
//! next attempt until the attempts or the deadline run out. Cancelling a task that is on a node asks
//! the node to stop; the task ends with the node's result or when its lease runs out.

use kernel::{ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How long a task turned down for quota or draining waits before it is offered again.
pub const RETRY_DELAY_MICROS: i64 = 10_000_000;

/// The closed set of node task kinds. There is no "run a command" kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    AutotracingRun,
    Profile,
    Tool,
}

impl TaskKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "autotracing_run" => Some(Self::AutotracingRun),
            "profile" => Some(Self::Profile),
            "tool" => Some(Self::Tool),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::AutotracingRun => "autotracing_run",
            Self::Profile => "profile",
            Self::Tool => "tool",
        }
    }
}

/// `(task_id, attempt)`: one offer of a task.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LeaseKey {
    pub task_id: Id,
    pub attempt: u32,
}

impl LeaseKey {
    /// `result_id = <task_id>/<attempt>`.
    pub fn result_id(&self) -> String {
        format!("{}/{}", self.task_id.as_str(), self.attempt)
    }

    pub fn parse_result_id(result_id: &str) -> Option<Self> {
        let (task_id, attempt) = result_id.rsplit_once('/')?;

        if task_id.is_empty() {
            return None;
        }

        Some(Self {
            task_id: Id::from_string(task_id),
            attempt: attempt.parse().ok()?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    /// Waiting to be offered.
    Queued,
    /// Offered to the node, not accepted yet.
    Offered,
    /// Accepted; the lease is renewed by heartbeats.
    Running,
    Succeeded,
    Failed,
    Cancelled,
    /// The lease ran out while the task ran.
    LeaseExpired,
    /// The deadline or the attempts ran out before the task ran.
    Expired,
    /// The node does not run this kind of task.
    Rejected,
}

impl TaskState {
    pub const ACTIVE: [Self; 3] = [Self::Queued, Self::Offered, Self::Running];

    pub fn is_active(self) -> bool {
        Self::ACTIVE.contains(&self)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Offered => "offered",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::LeaseExpired => "lease_expired",
            Self::Expired => "expired",
            Self::Rejected => "rejected",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::Queued,
            Self::Offered,
            Self::Running,
            Self::Succeeded,
            Self::Failed,
            Self::Cancelled,
            Self::LeaseExpired,
            Self::Expired,
            Self::Rejected,
        ]
        .into_iter()
        .find(|state| state.as_str() == value)
    }
}

/// How the node says a task ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Succeeded,
    Failed,
    Cancelled,
    LeaseExpired,
}

impl From<TaskStatus> for TaskState {
    fn from(status: TaskStatus) -> Self {
        match status {
            TaskStatus::Succeeded => Self::Succeeded,
            TaskStatus::Failed => Self::Failed,
            TaskStatus::Cancelled => Self::Cancelled,
            TaskStatus::LeaseExpired => Self::LeaseExpired,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    UnsupportedKind,
    Quota,
    Draining,
    Expired,
}

/// The result a node reported, recorded once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskOutcome {
    pub result_id: String,
    pub attempt: u32,
    pub status: TaskStatus,
    pub summary: String,
    pub artifact_refs: Vec<String>,
    pub received_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeTask {
    pub id: Id,
    pub org_id: Id,
    pub node_id: Id,
    pub kind: TaskKind,
    pub spec: Value,
    pub state: TaskState,
    pub attempt: u32,
    pub max_attempts: u32,
    /// Not offered before this point (a retry waits).
    pub available_at: TimestampMicros,
    pub lease_expires_at: Option<TimestampMicros>,
    pub deadline: TimestampMicros,
    pub cancel_requested_at: Option<TimestampMicros>,
    pub cancel_requested_by: Option<String>,
    pub result: Option<TaskOutcome>,
    /// Why the control plane ended the task, when the node did not.
    pub failure: Option<String>,
    pub created_by: String,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
    pub finished_at: Option<TimestampMicros>,
}

/// Something that happens to a task.
#[derive(Debug, Clone)]
pub enum TaskEvent {
    /// Hand the task to its node.
    Offer {
        lease_ttl_micros: i64,
    },
    Accept {
        attempt: u32,
        lease_ttl_micros: i64,
    },
    Reject {
        attempt: u32,
        reason: RejectReason,
    },
    /// The node still holds the lease (a heartbeat).
    Renew {
        attempt: u32,
        lease_ttl_micros: i64,
    },
    Result(ResultReport),
    Cancel {
        by: String,
    },
    /// Deadline and lease expiry.
    Expire,
    /// The node was revoked or enrolled again: it will never finish the task.
    NodeGone {
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct ResultReport {
    pub result_id: String,
    pub attempt: u32,
    pub status: TaskStatus,
    pub summary: String,
    pub artifact_refs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultOutcome {
    Recorded,
    /// The same result again: acknowledged, not recorded.
    Duplicate,
    /// A result of an attempt that is not the current one: acknowledged, not recorded.
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    Unchanged,
    Offered {
        lease_expires_at: TimestampMicros,
    },
    /// Accepted or renewed: the lease now ends here.
    Leased {
        lease_expires_at: TimestampMicros,
    },
    Requeued,
    CancelRequested,
    Finished(TaskState),
    /// The node holds a lease the control plane no longer gives it: tell it to cancel.
    Stale,
    Result(ResultOutcome),
}

impl Transition {
    /// Whether the task changed and has to be stored.
    pub fn changed(self) -> bool {
        !matches!(
            self,
            Self::Unchanged
                | Self::Stale
                | Self::Result(ResultOutcome::Duplicate | ResultOutcome::Ignored)
        )
    }
}

impl NodeTask {
    pub fn lease_key(&self) -> LeaseKey {
        LeaseKey {
            task_id: self.id.clone(),
            attempt: self.attempt,
        }
    }

    pub fn apply(&mut self, event: TaskEvent, now: TimestampMicros) -> Transition {
        let transition = match event {
            TaskEvent::Offer { lease_ttl_micros } => self.offer(lease_ttl_micros, now),
            TaskEvent::Accept {
                attempt,
                lease_ttl_micros,
            }
            | TaskEvent::Renew {
                attempt,
                lease_ttl_micros,
            } => self.lease(attempt, lease_ttl_micros, now),
            TaskEvent::Reject { attempt, reason } => self.reject(attempt, reason, now),
            TaskEvent::Result(report) => self.record(report, now),
            TaskEvent::Cancel { by } => self.cancel(by, now),
            TaskEvent::Expire => self.expire(now),
            TaskEvent::NodeGone { reason } if self.state.is_active() => {
                self.finish(TaskState::Cancelled, Some(reason), now)
            }
            TaskEvent::NodeGone { .. } => Transition::Unchanged,
        };

        if transition.changed() {
            self.updated_at = now;
        }

        transition
    }

    fn offer(&mut self, lease_ttl_micros: i64, now: TimestampMicros) -> Transition {
        if self.state != TaskState::Queued || self.available_at > now || self.deadline <= now {
            return Transition::Unchanged;
        }

        let lease_expires_at = self.lease_until(lease_ttl_micros, now);
        self.state = TaskState::Offered;
        self.lease_expires_at = Some(lease_expires_at);
        Transition::Offered { lease_expires_at }
    }

    fn lease(&mut self, attempt: u32, lease_ttl_micros: i64, now: TimestampMicros) -> Transition {
        let live = matches!(self.state, TaskState::Offered | TaskState::Running)
            && attempt == self.attempt
            && self.cancel_requested_at.is_none()
            && self.lease_expires_at.is_some_and(|expires| expires > now);

        if !live {
            return Transition::Stale;
        }

        // A renewal never shortens the lease.
        let lease_expires_at = self
            .lease_until(lease_ttl_micros, now)
            .max(self.lease_expires_at.unwrap_or(now));

        self.state = TaskState::Running;
        self.lease_expires_at = Some(lease_expires_at);
        Transition::Leased { lease_expires_at }
    }

    fn reject(&mut self, attempt: u32, reason: RejectReason, now: TimestampMicros) -> Transition {
        if attempt != self.attempt || !matches!(self.state, TaskState::Offered | TaskState::Running)
        {
            return Transition::Unchanged;
        }

        if self.cancel_requested_at.is_some() {
            return self.finish(TaskState::Cancelled, None, now);
        }

        match reason {
            RejectReason::UnsupportedKind => self.finish(
                TaskState::Rejected,
                Some(format!(
                    "the node does not run `{}` tasks",
                    self.kind.as_str()
                )),
                now,
            ),
            RejectReason::Quota | RejectReason::Draining | RejectReason::Expired => {
                self.requeue(now)
            }
        }
    }

    fn record(&mut self, report: ResultReport, now: TimestampMicros) -> Transition {
        if let Some(existing) = &self.result {
            return Transition::Result(if existing.result_id == report.result_id {
                ResultOutcome::Duplicate
            } else {
                ResultOutcome::Ignored
            });
        }

        let current = LeaseKey {
            task_id: self.id.clone(),
            attempt: self.attempt,
        };

        if report.attempt != self.attempt
            || report.result_id != current.result_id()
            || self.state == TaskState::Queued
        {
            return Transition::Result(ResultOutcome::Ignored);
        }

        self.result = Some(TaskOutcome {
            result_id: report.result_id,
            attempt: report.attempt,
            status: report.status,
            summary: report.summary,
            artifact_refs: report.artifact_refs,
            received_at: now,
        });

        self.state = report.status.into();
        self.lease_expires_at = None;
        self.finished_at.get_or_insert(now);
        Transition::Result(ResultOutcome::Recorded)
    }

    fn cancel(&mut self, by: String, now: TimestampMicros) -> Transition {
        match self.state {
            TaskState::Queued => self.finish(
                TaskState::Cancelled,
                Some(format!("cancelled by {by}")),
                now,
            ),
            TaskState::Offered | TaskState::Running if self.cancel_requested_at.is_none() => {
                self.cancel_requested_at = Some(now);
                self.cancel_requested_by = Some(by);
                Transition::CancelRequested
            }
            _ => Transition::Unchanged,
        }
    }

    fn expire(&mut self, now: TimestampMicros) -> Transition {
        let lease_over = self.lease_expires_at.is_none_or(|expires| expires <= now);

        match self.state {
            TaskState::Queued if self.deadline <= now => self.finish(
                TaskState::Expired,
                Some("the deadline passed before the node took the task".into()),
                now,
            ),
            TaskState::Offered | TaskState::Running
                if lease_over && self.cancel_requested_at.is_some() =>
            {
                self.finish(TaskState::Cancelled, None, now)
            }
            // Never accepted: the node is gone or did not answer; offer it again.
            TaskState::Offered if lease_over => self.requeue(now),
            TaskState::Running if lease_over => self.finish(
                TaskState::LeaseExpired,
                Some("the lease ran out before the node reported a result".into()),
                now,
            ),
            _ => Transition::Unchanged,
        }
    }

    fn requeue(&mut self, now: TimestampMicros) -> Transition {
        let retry_at = TimestampMicros(now.0.saturating_add(RETRY_DELAY_MICROS));

        if self.attempt >= self.max_attempts || retry_at >= self.deadline {
            return self.finish(
                TaskState::Expired,
                Some("no attempt was left before the deadline".into()),
                now,
            );
        }

        self.attempt += 1;
        self.state = TaskState::Queued;
        self.lease_expires_at = None;
        self.available_at = retry_at;
        Transition::Requeued
    }

    fn finish(
        &mut self,
        state: TaskState,
        failure: Option<String>,
        now: TimestampMicros,
    ) -> Transition {
        self.state = state;
        self.lease_expires_at = None;
        self.finished_at = Some(now);

        if failure.is_some() {
            self.failure = failure;
        }

        Transition::Finished(state)
    }

    fn lease_until(&self, lease_ttl_micros: i64, now: TimestampMicros) -> TimestampMicros {
        TimestampMicros(now.0.saturating_add(lease_ttl_micros)).min(self.deadline)
    }
}
