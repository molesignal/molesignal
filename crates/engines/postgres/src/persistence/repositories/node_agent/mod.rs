// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Postgres storage of the Node Agent inventory, enrollment tokens and tasks
//! ([`domain::node_agent::NodeAgentRepository`]). Task transitions are the domain's
//! [`domain::node_agent::NodeTask::apply`], run under a row lock.

mod nodes;
mod rows;
mod tasks;

use async_trait::async_trait;
use domain::node_agent::{
    ConfigStatus, EnrollmentToken, LeaseKey, NodeAgent, NodeAgentRepository, NodeTask,
    SessionReport, TaskEvent, TaskFilter, TaskKind, TaskQuota, Transition,
};
use kernel::{Result, ids::Id, time::TimestampMicros};
use serde_json::{Value, json};
use sqlx::PgPool;

use self::nodes::Bind;
use super::org_settings::{self, sections};

pub struct PgNodeAgentRepository {
    pool: PgPool,
}

impl PgNodeAgentRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl NodeAgentRepository for PgNodeAgentRepository {
    async fn create_enrollment_token(
        &self,
        token: &EnrollmentToken,
        token_hash: &[u8],
    ) -> Result<()> {
        self.insert_token(token, token_hash).await
    }

    async fn list_enrollment_tokens(&self, org_id: &Id) -> Result<Vec<EnrollmentToken>> {
        self.tokens(org_id).await
    }

    async fn revoke_enrollment_token(
        &self,
        org_id: &Id,
        token_id: &Id,
        now: TimestampMicros,
    ) -> Result<EnrollmentToken> {
        self.revoke_token(org_id, token_id, now).await
    }

    async fn enrollment_token_by_hash(&self, token_hash: &[u8]) -> Result<Option<EnrollmentToken>> {
        self.token_by_hash(token_hash).await
    }

    async fn node_by_host(&self, org_id: &Id, host_id: &str) -> Result<Option<NodeAgent>> {
        self.node_where("org_id = $1 AND host_id = $2", &[org_id.as_str(), host_id])
            .await
    }

    async fn enroll_node(
        &self,
        token_hash: &[u8],
        node: &NodeAgent,
        now: TimestampMicros,
    ) -> Result<NodeAgent> {
        self.enroll(token_hash, node, now).await
    }

    async fn node_by_certificate_serial(&self, serial: &str) -> Result<Option<NodeAgent>> {
        self.node_where(
            "certificate_serial = $1 OR previous_certificate_serial = $1",
            &[serial],
        )
        .await
    }

    async fn get_node(&self, org_id: &Id, node_id: &Id) -> Result<NodeAgent> {
        self.node_where(
            "org_id = $1 AND id = $2",
            &[org_id.as_str(), node_id.as_str()],
        )
        .await?
        .ok_or_else(|| kernel::Error::not_found(format!("node `{}` not found", node_id.as_str())))
    }

    async fn list_nodes(&self, org_id: &Id) -> Result<Vec<NodeAgent>> {
        self.nodes(org_id).await
    }

    async fn record_session(
        &self,
        org_id: &Id,
        node_id: &Id,
        report: &SessionReport,
        now: TimestampMicros,
    ) -> Result<NodeAgent> {
        self.session(org_id, node_id, report, now).await
    }

    async fn rotate_certificate(
        &self,
        org_id: &Id,
        node_id: &Id,
        serial: &str,
        expires_at: TimestampMicros,
        now: TimestampMicros,
    ) -> Result<NodeAgent> {
        self.update_node(
            org_id,
            node_id,
            now,
            "previous_certificate_serial = certificate_serial, certificate_serial = $4,
             certificate_expires_at = $5",
            vec![Bind::Text(serial.into()), Bind::Time(Some(expires_at))],
        )
        .await
    }

    async fn set_drain(
        &self,
        org_id: &Id,
        node_id: &Id,
        deadline: Option<TimestampMicros>,
        now: TimestampMicros,
    ) -> Result<NodeAgent> {
        let assignments = if deadline.is_some() {
            "drain_requested_at = COALESCE(drain_requested_at, $3), drain_deadline = $4,
             drained_at = NULL"
        } else {
            "drain_requested_at = NULL, drain_deadline = NULL, drained_at = NULL"
        };

        let binds = deadline
            .map(|deadline| vec![Bind::Time(Some(deadline))])
            .unwrap_or_default();

        self.update_node(org_id, node_id, now, assignments, binds)
            .await
    }

    async fn mark_drained(&self, org_id: &Id, node_id: &Id, now: TimestampMicros) -> Result<()> {
        self.update_node(
            org_id,
            node_id,
            now,
            "drained_at = CASE WHEN drain_requested_at IS NULL THEN NULL ELSE $3 END",
            Vec::new(),
        )
        .await
        .map(|_| ())
    }

    async fn revoke_node(
        &self,
        org_id: &Id,
        node_id: &Id,
        reason: &str,
        now: TimestampMicros,
    ) -> Result<(NodeAgent, u64)> {
        self.revoke(org_id, node_id, reason, now).await
    }

    async fn set_desired_config(
        &self,
        org_id: &Id,
        node_id: &Id,
        config: Value,
        updated_by: &str,
        now: TimestampMicros,
    ) -> Result<NodeAgent> {
        // The revision is the stored one plus one, taken in the same statement.
        self.update_node(
            org_id,
            node_id,
            now,
            "desired_config = JSONB_BUILD_OBJECT(
                'revision', COALESCE((desired_config ->> 'revision')::BIGINT, 0) + 1,
                'config', $4::JSONB -> 'config',
                'updatedBy', $4::JSONB -> 'updatedBy',
                'updatedAt', $4::JSONB -> 'updatedAt')",
            vec![Bind::Json(json!({
                "config": config,
                "updatedBy": updated_by,
                "updatedAt": now.0,
            }))],
        )
        .await
    }

    async fn record_config_status(
        &self,
        org_id: &Id,
        node_id: &Id,
        status: &ConfigStatus,
    ) -> Result<()> {
        self.config_status(org_id, node_id, status).await
    }

    async fn task_quota(&self, org_id: &Id) -> Result<TaskQuota> {
        Ok(
            org_settings::read::<TaskQuota>(&self.pool, &org_id.0, sections::NODE_AGENT_TASK_QUOTA)
                .await?
                .map(|section| section.value)
                .unwrap_or_default(),
        )
    }

    async fn set_task_quota(
        &self,
        org_id: &Id,
        quota: &TaskQuota,
        updated_by: &str,
        now: TimestampMicros,
    ) -> Result<()> {
        org_settings::write(
            &self.pool,
            &org_id.0,
            sections::NODE_AGENT_TASK_QUOTA,
            quota,
            Some(updated_by),
            now,
        )
        .await
    }

    async fn create_task(&self, task: &NodeTask, quota: &TaskQuota) -> Result<NodeTask> {
        self.insert_task(task, quota).await
    }

    async fn get_task(&self, org_id: &Id, task_id: &Id) -> Result<NodeTask> {
        self.task(org_id, task_id).await
    }

    async fn list_tasks(&self, org_id: &Id, filter: &TaskFilter) -> Result<Vec<NodeTask>> {
        self.tasks(org_id, filter).await
    }

    async fn apply_task_event(
        &self,
        org_id: &Id,
        node_id: Option<&Id>,
        task_id: &Id,
        event: TaskEvent,
        now: TimestampMicros,
    ) -> Result<Option<(NodeTask, Transition)>> {
        self.transition(org_id, node_id, task_id, event, now).await
    }

    async fn offer_tasks(
        &self,
        org_id: &Id,
        node_id: &Id,
        kinds: &[TaskKind],
        max_in_flight: u32,
        lease_ttl_micros: i64,
        now: TimestampMicros,
    ) -> Result<Vec<NodeTask>> {
        self.offer(org_id, node_id, kinds, max_in_flight, lease_ttl_micros, now)
            .await
    }

    async fn cancel_requested_leases(&self, org_id: &Id, node_id: &Id) -> Result<Vec<LeaseKey>> {
        self.cancel_requested(org_id, node_id).await
    }

    async fn expire_due_tasks(
        &self,
        org_id: Option<&Id>,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<NodeTask>> {
        self.expire(org_id, now, limit).await
    }
}
