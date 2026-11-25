// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Node tasks: creation under the quota, transitions under a row lock, offers and expiry.

use domain::node_agent::{
    LeaseKey, NodeTask, QuotaUsage, TaskEvent, TaskFilter, TaskKind, TaskQuota, Transition,
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use sqlx::{Row, types::Json};

use super::{
    PgNodeAgentRepository,
    rows::{ACTIVE_STATES, TASK_COLUMNS, save_task, task_row},
};
use crate::persistence::sqlx_err;

impl PgNodeAgentRepository {
    pub(super) async fn insert_task(&self, task: &NodeTask, quota: &TaskQuota) -> Result<NodeTask> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        // One organization creates tasks one at a time, so two requests cannot both take the
        // last slot.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('node_agent_tasks:' || $1, 0))")
            .bind(&task.org_id.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;

        let node = sqlx::query(
            "SELECT revoked_at IS NOT NULL AS revoked, drain_requested_at IS NOT NULL AS draining
             FROM node_agents WHERE org_id = $1 AND id = $2 FOR UPDATE",
        )
        .bind(&task.org_id.0)
        .bind(&task.node_id.0)
        .fetch_optional(&mut *tx)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found(format!("node `{}` not found", task.node_id.as_str())))?;

        if node.try_get::<bool, _>("revoked").map_err(sqlx_err)? {
            return Err(Error::conflict("the node is revoked"));
        }

        if node.try_get::<bool, _>("draining").map_err(sqlx_err)? {
            return Err(Error::conflict(
                "the node is draining and takes no new tasks",
            ));
        }

        let window_start = TimestampMicros(task.created_at.0 - TaskQuota::WINDOW_MICROS);

        let usage = sqlx::query(&format!(
            "SELECT
                COUNT(*) FILTER (WHERE state IN {ACTIVE_STATES}) AS org_active,
                COUNT(*) FILTER (WHERE state IN {ACTIVE_STATES} AND node_id = $2) AS node_active,
                COUNT(*) FILTER (WHERE created_at > $3) AS org_created,
                COUNT(*) FILTER (WHERE created_at > $3 AND node_id = $2) AS node_created
             FROM node_agent_tasks
             WHERE org_id = $1 AND (state IN {ACTIVE_STATES} OR created_at > $3)"
        ))
        .bind(&task.org_id.0)
        .bind(&task.node_id.0)
        .bind(window_start)
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        let count = |column: &str| -> Result<u32> {
            let value: i64 = usage.try_get(column).map_err(sqlx_err)?;
            Ok(u32::try_from(value).unwrap_or(u32::MAX))
        };

        quota.admit(QuotaUsage {
            org_active: count("org_active")?,
            node_active: count("node_active")?,
            org_created_last_hour: count("org_created")?,
            node_created_last_hour: count("node_created")?,
        })?;

        let row = sqlx::query(&format!(
            "INSERT INTO node_agent_tasks
                (id, org_id, node_id, kind, spec, state, attempt, max_attempts, available_at,
                 deadline, created_by, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $12)
             RETURNING {TASK_COLUMNS}"
        ))
        .bind(&task.id.0)
        .bind(&task.org_id.0)
        .bind(&task.node_id.0)
        .bind(task.kind.as_str())
        .bind(Json(&task.spec))
        .bind(task.state.as_str())
        .bind(task.attempt as i32)
        .bind(task.max_attempts as i32)
        .bind(task.available_at)
        .bind(task.deadline)
        .bind(&task.created_by)
        .bind(task.created_at)
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        task_row(&row)
    }

    pub(super) async fn task(&self, org_id: &Id, task_id: &Id) -> Result<NodeTask> {
        let row = sqlx::query(&format!(
            "SELECT {TASK_COLUMNS} FROM node_agent_tasks WHERE org_id = $1 AND id = $2"
        ))
        .bind(&org_id.0)
        .bind(&task_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found(format!("task `{}` not found", task_id.as_str())))?;

        task_row(&row)
    }

    pub(super) async fn tasks(&self, org_id: &Id, filter: &TaskFilter) -> Result<Vec<NodeTask>> {
        let states = filter
            .states
            .iter()
            .map(|state| state.as_str().to_string())
            .collect::<Vec<_>>();

        sqlx::query(&format!(
            "SELECT {TASK_COLUMNS} FROM node_agent_tasks
             WHERE org_id = $1 AND ($2::TEXT IS NULL OR node_id = $2)
               AND (CARDINALITY($3::TEXT[]) = 0 OR state = ANY($3))
             ORDER BY created_at DESC, id DESC LIMIT $4"
        ))
        .bind(&org_id.0)
        .bind(filter.node_id.as_ref().map(|id| id.0.clone()))
        .bind(states)
        .bind(i64::from(filter.limit.clamp(1, 1000)))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?
        .iter()
        .map(task_row)
        .collect()
    }

    pub(super) async fn transition(
        &self,
        org_id: &Id,
        node_id: Option<&Id>,
        task_id: &Id,
        event: TaskEvent,
        now: TimestampMicros,
    ) -> Result<Option<(NodeTask, Transition)>> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        let Some(row) = sqlx::query(&format!(
            "SELECT {TASK_COLUMNS} FROM node_agent_tasks
             WHERE org_id = $1 AND id = $2 AND ($3::TEXT IS NULL OR node_id = $3)
             FOR UPDATE"
        ))
        .bind(&org_id.0)
        .bind(&task_id.0)
        .bind(node_id.map(|id| id.0.clone()))
        .fetch_optional(&mut *tx)
        .await
        .map_err(sqlx_err)?
        else {
            return Ok(None);
        };

        let mut task = task_row(&row)?;
        let transition = task.apply(event, now);

        if transition.changed() {
            save_task(&mut tx, &task).await?;
        }

        tx.commit().await.map_err(sqlx_err)?;
        Ok(Some((task, transition)))
    }

    pub(super) async fn offer(
        &self,
        org_id: &Id,
        node_id: &Id,
        kinds: &[TaskKind],
        max_in_flight: u32,
        lease_ttl_micros: i64,
        now: TimestampMicros,
    ) -> Result<Vec<NodeTask>> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        // Offers to one node are made one at a time, so the in-flight count holds.
        let takes_tasks: Option<bool> = sqlx::query_scalar(
            "SELECT revoked_at IS NULL AND drain_requested_at IS NULL
             FROM node_agents WHERE org_id = $1 AND id = $2 FOR UPDATE",
        )
        .bind(&org_id.0)
        .bind(&node_id.0)
        .fetch_optional(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        if takes_tasks != Some(true) || kinds.is_empty() {
            return Ok(Vec::new());
        }

        let in_flight: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM node_agent_tasks
             WHERE org_id = $1 AND node_id = $2 AND state IN ('offered', 'running')",
        )
        .bind(&org_id.0)
        .bind(&node_id.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        let slots = i64::from(max_in_flight).saturating_sub(in_flight);

        if slots <= 0 {
            return Ok(Vec::new());
        }

        let kinds = kinds
            .iter()
            .map(|kind| kind.as_str().to_string())
            .collect::<Vec<_>>();

        let rows = sqlx::query(&format!(
            "SELECT {TASK_COLUMNS} FROM node_agent_tasks
             WHERE org_id = $1 AND node_id = $2 AND state = 'queued' AND available_at <= $3
               AND deadline > $3 AND kind = ANY($4)
             ORDER BY created_at, id LIMIT $5
             FOR UPDATE SKIP LOCKED"
        ))
        .bind(&org_id.0)
        .bind(&node_id.0)
        .bind(now)
        .bind(kinds)
        .bind(slots)
        .fetch_all(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        let mut offered = Vec::with_capacity(rows.len());

        for row in rows {
            let mut task = task_row(&row)?;

            if task
                .apply(TaskEvent::Offer { lease_ttl_micros }, now)
                .changed()
            {
                save_task(&mut tx, &task).await?;
                offered.push(task);
            }
        }

        tx.commit().await.map_err(sqlx_err)?;
        Ok(offered)
    }

    pub(super) async fn cancel_requested(
        &self,
        org_id: &Id,
        node_id: &Id,
    ) -> Result<Vec<LeaseKey>> {
        let rows = sqlx::query(
            "SELECT id, attempt FROM node_agent_tasks
             WHERE org_id = $1 AND node_id = $2 AND state IN ('offered', 'running')
               AND cancel_requested_at IS NOT NULL",
        )
        .bind(&org_id.0)
        .bind(&node_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter()
            .map(|row| {
                let attempt: i32 = row.try_get("attempt").map_err(sqlx_err)?;

                Ok(LeaseKey {
                    task_id: Id(row.try_get("id").map_err(sqlx_err)?),
                    attempt: u32::try_from(attempt).unwrap_or_default(),
                })
            })
            .collect()
    }

    pub(super) async fn expire(
        &self,
        org_id: Option<&Id>,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<NodeTask>> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        let rows = sqlx::query(&format!(
            "SELECT {TASK_COLUMNS} FROM node_agent_tasks
             WHERE ($1::TEXT IS NULL OR org_id = $1)
               AND ((state = 'queued' AND deadline <= $2)
                    OR (state IN ('offered', 'running') AND lease_expires_at <= $2))
             ORDER BY updated_at LIMIT $3
             FOR UPDATE SKIP LOCKED"
        ))
        .bind(org_id.map(|id| id.0.clone()))
        .bind(now)
        .bind(i64::from(limit.clamp(1, 10_000)))
        .fetch_all(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        let mut changed = Vec::new();

        for row in rows {
            let mut task = task_row(&row)?;

            if task.apply(TaskEvent::Expire, now).changed() {
                save_task(&mut tx, &task).await?;
                changed.push(task);
            }
        }

        tx.commit().await.map_err(sqlx_err)?;
        Ok(changed)
    }
}
