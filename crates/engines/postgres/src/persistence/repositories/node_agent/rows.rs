// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Rows of `node_agents`, `node_agent_enrollment_tokens` and `node_agent_tasks` to domain values.

use std::collections::BTreeMap;

use domain::node_agent::{
    Capabilities, CollectorState, ConfigStatus, DesiredConfig, EnrollmentToken, Health, NodeAgent,
    NodeTask, TaskKind, TaskOutcome, TaskState,
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use sqlx::{PgConnection, Row, postgres::PgRow, types::Json};

use crate::persistence::sqlx_err;

pub(super) const TOKEN_COLUMNS: &str = "id, org_id, description, labels, max_uses, uses, \
     expires_at, revoked_at, last_used_at, created_by, created_at";

pub(super) const NODE_COLUMNS: &str = "id, org_id, host_id, boot_id, hostname, labels, \
     agent_version, protocol_version, capabilities, certificate_serial, \
     previous_certificate_serial, certificate_expires_at, enrollment_token_id, health, \
     collector_states, applied_config_revision, desired_config, config_status, last_heartbeat_at, \
     last_connected_at, drain_requested_at, drain_deadline, drained_at, revoked_at, revoke_reason, \
     enrolled_at, created_at, updated_at";

pub(super) const TASK_COLUMNS: &str = "id, org_id, node_id, kind, spec, state, attempt, \
     max_attempts, available_at, lease_expires_at, deadline, cancel_requested_at, \
     cancel_requested_by, result, failure, created_by, created_at, updated_at, finished_at";

pub(super) const ACTIVE_STATES: &str = "('queued', 'offered', 'running')";

fn get<'r, T>(row: &'r PgRow, column: &str) -> Result<T>
where
    T: sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    row.try_get(column).map_err(sqlx_err)
}

fn count(row: &PgRow, column: &str) -> Result<u32> {
    let value: i32 = get(row, column)?;
    u32::try_from(value).map_err(|_| Error::internal(format!("negative `{column}`")))
}

fn revision(row: &PgRow, column: &str) -> Result<u64> {
    let value: i64 = get(row, column)?;
    u64::try_from(value).map_err(|_| Error::internal(format!("negative `{column}`")))
}

pub(super) fn token_row(row: &PgRow) -> Result<EnrollmentToken> {
    Ok(EnrollmentToken {
        id: Id(get(row, "id")?),
        org_id: Id(get(row, "org_id")?),
        description: get(row, "description")?,
        labels: get::<Json<BTreeMap<String, String>>>(row, "labels")?.0,
        max_uses: count(row, "max_uses")?,
        uses: count(row, "uses")?,
        expires_at: get(row, "expires_at")?,
        revoked_at: get(row, "revoked_at")?,
        last_used_at: get(row, "last_used_at")?,
        created_by: get(row, "created_by")?,
        created_at: get(row, "created_at")?,
    })
}

pub(super) fn node_row(row: &PgRow) -> Result<NodeAgent> {
    Ok(NodeAgent {
        id: Id(get(row, "id")?),
        org_id: Id(get(row, "org_id")?),
        host_id: get(row, "host_id")?,
        boot_id: get(row, "boot_id")?,
        hostname: get(row, "hostname")?,
        labels: get::<Json<BTreeMap<String, String>>>(row, "labels")?.0,
        agent_version: get(row, "agent_version")?,
        protocol_version: get(row, "protocol_version")?,
        capabilities: get::<Json<Capabilities>>(row, "capabilities")?.0,
        certificate_serial: get(row, "certificate_serial")?,
        previous_certificate_serial: get(row, "previous_certificate_serial")?,
        certificate_expires_at: get(row, "certificate_expires_at")?,
        enrollment_token_id: get::<Option<String>>(row, "enrollment_token_id")?.map(Id),
        health: get::<Json<Health>>(row, "health")?.0,
        collector_states: get::<Json<Vec<CollectorState>>>(row, "collector_states")?.0,
        applied_config_revision: revision(row, "applied_config_revision")?,
        desired_config: get::<Option<Json<DesiredConfig>>>(row, "desired_config")?
            .map(|value| value.0),
        config_status: get::<Option<Json<ConfigStatus>>>(row, "config_status")?
            .map(|value| value.0),
        last_heartbeat_at: get(row, "last_heartbeat_at")?,
        last_connected_at: get(row, "last_connected_at")?,
        drain_requested_at: get(row, "drain_requested_at")?,
        drain_deadline: get(row, "drain_deadline")?,
        drained_at: get(row, "drained_at")?,
        revoked_at: get(row, "revoked_at")?,
        revoke_reason: get(row, "revoke_reason")?,
        enrolled_at: get(row, "enrolled_at")?,
        created_at: get(row, "created_at")?,
        updated_at: get(row, "updated_at")?,
    })
}

pub(super) fn task_row(row: &PgRow) -> Result<NodeTask> {
    let kind: String = get(row, "kind")?;
    let state: String = get(row, "state")?;

    Ok(NodeTask {
        id: Id(get(row, "id")?),
        org_id: Id(get(row, "org_id")?),
        node_id: Id(get(row, "node_id")?),
        kind: TaskKind::parse(&kind)
            .ok_or_else(|| Error::internal(format!("unknown node task kind `{kind}`")))?,
        spec: get::<Json<serde_json::Value>>(row, "spec")?.0,
        state: TaskState::parse(&state)
            .ok_or_else(|| Error::internal(format!("unknown node task state `{state}`")))?,
        attempt: count(row, "attempt")?,
        max_attempts: count(row, "max_attempts")?,
        available_at: get(row, "available_at")?,
        lease_expires_at: get(row, "lease_expires_at")?,
        deadline: get(row, "deadline")?,
        cancel_requested_at: get(row, "cancel_requested_at")?,
        cancel_requested_by: get(row, "cancel_requested_by")?,
        result: get::<Option<Json<TaskOutcome>>>(row, "result")?.map(|value| value.0),
        failure: get(row, "failure")?,
        created_by: get(row, "created_by")?,
        created_at: get(row, "created_at")?,
        updated_at: get(row, "updated_at")?,
        finished_at: get(row, "finished_at")?,
    })
}

/// Stores what a transition may have changed.
pub(super) async fn save_task(connection: &mut PgConnection, task: &NodeTask) -> Result<()> {
    sqlx::query(
        "UPDATE node_agent_tasks SET
            state = $3, attempt = $4, available_at = $5, lease_expires_at = $6,
            cancel_requested_at = $7, cancel_requested_by = $8, result = $9, failure = $10,
            updated_at = $11, finished_at = $12
         WHERE org_id = $1 AND id = $2",
    )
    .bind(&task.org_id.0)
    .bind(&task.id.0)
    .bind(task.state.as_str())
    .bind(task.attempt as i32)
    .bind(task.available_at)
    .bind(task.lease_expires_at)
    .bind(task.cancel_requested_at)
    .bind(&task.cancel_requested_by)
    .bind(task.result.as_ref().map(Json))
    .bind(&task.failure)
    .bind(task.updated_at)
    .bind(task.finished_at)
    .execute(connection)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

/// Ends the active tasks of a node that will never finish them.
pub(super) async fn end_node_tasks(
    connection: &mut PgConnection,
    org_id: &Id,
    node_id: &Id,
    reason: &str,
    now: TimestampMicros,
) -> Result<u64> {
    use domain::node_agent::TaskEvent;

    let rows = sqlx::query(&format!(
        "SELECT {TASK_COLUMNS} FROM node_agent_tasks
         WHERE org_id = $1 AND node_id = $2 AND state IN {ACTIVE_STATES}
         FOR UPDATE"
    ))
    .bind(&org_id.0)
    .bind(&node_id.0)
    .fetch_all(&mut *connection)
    .await
    .map_err(sqlx_err)?;

    let mut ended = 0;

    for row in rows {
        let mut task = task_row(&row)?;

        let transition = task.apply(
            TaskEvent::NodeGone {
                reason: reason.into(),
            },
            now,
        );

        if transition.changed() {
            save_task(connection, &task).await?;
            ended += 1;
        }
    }

    Ok(ended)
}
