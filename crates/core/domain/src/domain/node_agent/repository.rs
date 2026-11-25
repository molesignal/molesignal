// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use kernel::{Result, ids::Id, time::TimestampMicros};
use serde_json::Value;

use super::{
    ConfigStatus, EnrollmentToken, LeaseKey, NodeAgent, NodeTask, SessionReport, TaskEvent,
    TaskKind, TaskQuota, TaskState, Transition,
};

#[derive(Debug, Clone, Default)]
pub struct TaskFilter {
    pub node_id: Option<Id>,
    pub states: Vec<TaskState>,
    pub limit: u32,
}

/// Storage of the Node Agent inventory, enrollment tokens and tasks. Every read and write that
/// names an organization stays inside it; only [`Self::node_by_certificate_serial`] and the
/// expiry sweep cross organizations, because a certificate names its organization by itself.
///
/// Task transitions are [`NodeTask::apply`]: an implementation loads the task under a lock,
/// applies the event and stores what changed.
#[async_trait]
pub trait NodeAgentRepository: Send + Sync {
    async fn create_enrollment_token(
        &self,
        token: &EnrollmentToken,
        token_hash: &[u8],
    ) -> Result<()>;
    async fn list_enrollment_tokens(&self, org_id: &Id) -> Result<Vec<EnrollmentToken>>;
    async fn revoke_enrollment_token(
        &self,
        org_id: &Id,
        token_id: &Id,
        now: TimestampMicros,
    ) -> Result<EnrollmentToken>;
    async fn enrollment_token_by_hash(&self, token_hash: &[u8]) -> Result<Option<EnrollmentToken>>;

    async fn node_by_host(&self, org_id: &Id, host_id: &str) -> Result<Option<NodeAgent>>;
    /// Uses up one enrollment of the token, which must still be usable at `now`, and stores the
    /// node: a new one, or the node already enrolled from the same host with its new certificate,
    /// its revocation and drain lifted, and its active tasks cancelled.
    async fn enroll_node(
        &self,
        token_hash: &[u8],
        node: &NodeAgent,
        now: TimestampMicros,
    ) -> Result<NodeAgent>;
    /// The node a client certificate serial (current or just rotated away) identifies, revoked or
    /// not.
    async fn node_by_certificate_serial(&self, serial: &str) -> Result<Option<NodeAgent>>;
    async fn get_node(&self, org_id: &Id, node_id: &Id) -> Result<NodeAgent>;
    async fn list_nodes(&self, org_id: &Id) -> Result<Vec<NodeAgent>>;
    async fn record_session(
        &self,
        org_id: &Id,
        node_id: &Id,
        report: &SessionReport,
        now: TimestampMicros,
    ) -> Result<NodeAgent>;
    /// Stores a new certificate; the replaced one stays accepted until a session uses the new one.
    async fn rotate_certificate(
        &self,
        org_id: &Id,
        node_id: &Id,
        serial: &str,
        expires_at: TimestampMicros,
        now: TimestampMicros,
    ) -> Result<NodeAgent>;
    /// `Some(deadline)` starts a drain; `None` lifts it.
    async fn set_drain(
        &self,
        org_id: &Id,
        node_id: &Id,
        deadline: Option<TimestampMicros>,
        now: TimestampMicros,
    ) -> Result<NodeAgent>;
    async fn mark_drained(&self, org_id: &Id, node_id: &Id, now: TimestampMicros) -> Result<()>;
    /// Revokes the node and cancels its active tasks; returns the node and how many tasks ended.
    async fn revoke_node(
        &self,
        org_id: &Id,
        node_id: &Id,
        reason: &str,
        now: TimestampMicros,
    ) -> Result<(NodeAgent, u64)>;
    /// Stores the configuration as the next revision.
    async fn set_desired_config(
        &self,
        org_id: &Id,
        node_id: &Id,
        config: Value,
        updated_by: &str,
        now: TimestampMicros,
    ) -> Result<NodeAgent>;
    async fn record_config_status(
        &self,
        org_id: &Id,
        node_id: &Id,
        status: &ConfigStatus,
    ) -> Result<()>;

    async fn task_quota(&self, org_id: &Id) -> Result<TaskQuota>;
    async fn set_task_quota(
        &self,
        org_id: &Id,
        quota: &TaskQuota,
        updated_by: &str,
        now: TimestampMicros,
    ) -> Result<()>;

    /// Stores a new task if the node takes tasks (enrolled in the organization, not revoked, not
    /// draining) and the quota admits it, both decided atomically with the insert.
    async fn create_task(&self, task: &NodeTask, quota: &TaskQuota) -> Result<NodeTask>;
    async fn get_task(&self, org_id: &Id, task_id: &Id) -> Result<NodeTask>;
    async fn list_tasks(&self, org_id: &Id, filter: &TaskFilter) -> Result<Vec<NodeTask>>;
    /// Applies `event` to a task of the organization (and of `node_id`, when given). `None` when
    /// there is no such task.
    async fn apply_task_event(
        &self,
        org_id: &Id,
        node_id: Option<&Id>,
        task_id: &Id,
        event: TaskEvent,
        now: TimestampMicros,
    ) -> Result<Option<(NodeTask, Transition)>>;
    /// Offers queued tasks of `kinds` to the node, so that at most `max_in_flight` of its tasks
    /// are offered or running.
    async fn offer_tasks(
        &self,
        org_id: &Id,
        node_id: &Id,
        kinds: &[TaskKind],
        max_in_flight: u32,
        lease_ttl_micros: i64,
        now: TimestampMicros,
    ) -> Result<Vec<NodeTask>>;
    /// Leases on the node whose task was asked to cancel.
    async fn cancel_requested_leases(&self, org_id: &Id, node_id: &Id) -> Result<Vec<LeaseKey>>;
    /// Applies [`TaskEvent::Expire`] to up to `limit` tasks whose deadline or lease has passed, in
    /// one organization or in all. Returns the tasks that changed.
    async fn expire_due_tasks(
        &self,
        org_id: Option<&Id>,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<NodeTask>>;
}
