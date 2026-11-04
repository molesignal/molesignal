// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;

use super::{
    PgSyntheticRepository,
    codec::{TASK_COLS, row_to_task},
};
use crate::{
    domain::synthetics::{ProbeAgent, ProbeTask, SyntheticTaskRepository},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[async_trait]
impl SyntheticTaskRepository for PgSyntheticRepository {
    async fn create_tasks(&self, tasks: Vec<ProbeTask>) -> Result<Vec<ProbeTask>> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        let mut persisted = Vec::with_capacity(tasks.len());

        for task in tasks {
            let sql = format!(
                "INSERT INTO synthetic_probe_tasks
                    (id, organization_id, monitor_id, monitor_revision_id, location_id, spec,
                     is_test, state, scheduled_at, deadline_at, timeout_millis,
                     max_attempts, required_feature_level, inject_trace_context,
                     traceroute_on_failure, lease_agent_id, lease_token_hash,
                     leased_until, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7,
                         CASE WHEN $7 OR NOT EXISTS (
                             SELECT 1 FROM synthetic_probe_tasks existing
                             WHERE existing.organization_id = $2
                               AND existing.monitor_id = $3 AND existing.location_id = $5
                               AND NOT existing.is_test
                               AND existing.state IN ('pending', 'leased', 'running')
                         ) THEN 'pending' ELSE 'skipped' END,
                         $8, $9, $10, $11, $14, $15, $16,
                         NULL, NULL, NULL, $12, $13)
                 ON CONFLICT (monitor_revision_id, location_id, scheduled_at, is_test)
                 DO UPDATE SET id = synthetic_probe_tasks.id
                 RETURNING {TASK_COLS}"
            );

            let row = sqlx::query(&sql)
                .bind(&task.id.0)
                .bind(&task.organization_id.0)
                .bind(&task.monitor_id.0)
                .bind(&task.monitor_revision_id.0)
                .bind(&task.location_id.0)
                .bind(sqlx::types::Json(&task.spec))
                .bind(task.is_test)
                .bind(task.scheduled_at)
                .bind(task.deadline_at)
                .bind(task.timeout_millis as i32)
                .bind(task.max_attempts as i16)
                .bind(task.created_at)
                .bind(task.updated_at)
                .bind(i32::try_from(task.spec.required_feature_level()).unwrap_or(i32::MAX))
                .bind(task.inject_trace_context)
                .bind(task.traceroute_on_failure)
                .fetch_one(&mut *transaction)
                .await
                .map_err(super::sqlx_err)?;

            persisted.push(row_to_task(row)?);
        }

        transaction.commit().await.map_err(super::sqlx_err)?;
        Ok(persisted)
    }

    async fn lease_task(
        &self,
        agent: &ProbeAgent,
        now: TimestampMicros,
        leased_until: TimestampMicros,
        lease_token_hash: String,
    ) -> Result<Option<ProbeTask>> {
        if !matches!(
            agent.status,
            crate::domain::synthetics::AgentStatus::Online
                | crate::domain::synthetics::AgentStatus::Degraded
        ) {
            return Err(Error::conflict("Probe Agent is not accepting tasks"));
        }

        let sql = format!(
            "WITH candidate AS (
                 SELECT task.id AS task_id
                 FROM synthetic_probe_tasks task
                 JOIN synthetic_probe_locations location ON location.id = task.location_id
                 JOIN synthetic_monitor_revisions revision
                   ON revision.organization_id = task.organization_id
                  AND revision.id = task.monitor_revision_id
                 WHERE task.location_id = $1 AND task.state = 'pending'
                   AND task.scheduled_at <= $3 AND task.deadline_at > $3
                   AND location.organization_id IS NOT DISTINCT FROM $2
                   AND revision.spec->>'kind' = ANY($7)
                   AND task.required_feature_level <= $8
                 ORDER BY task.scheduled_at, task.id
                 LIMIT 1 FOR UPDATE OF task SKIP LOCKED
             )
             UPDATE synthetic_probe_tasks task
             SET state = 'leased', lease_agent_id = $4, lease_token_hash = $5,
                 leased_until = $6, updated_at = $3
             FROM candidate
             WHERE task.id = candidate.task_id
             RETURNING {TASK_COLS}"
        );

        sqlx::query(&sql)
            .bind(&agent.location_id.0)
            .bind(agent.organization_id.as_ref().map(Id::as_str))
            .bind(now)
            .bind(&agent.id.0)
            .bind(lease_token_hash)
            .bind(leased_until)
            .bind(
                agent
                    .capabilities
                    .iter()
                    .map(|capability| capability.as_str().to_string())
                    .collect::<Vec<_>>(),
            )
            .bind(i32::try_from(agent.feature_level).unwrap_or(i32::MAX))
            .fetch_optional(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .map(row_to_task)
            .transpose()
    }

    async fn complete_task(
        &self,
        org_id: &Id,
        task_id: &Id,
        agent_id: Option<&Id>,
        now: TimestampMicros,
    ) -> Result<()> {
        let rows = sqlx::query(
            "UPDATE synthetic_probe_tasks
             SET state = 'completed', updated_at = $4
             WHERE organization_id = $1 AND id = $2
               AND lease_agent_id IS NOT DISTINCT FROM $3
               AND state IN ('leased', 'running', 'completed')",
        )
        .bind(&org_id.0)
        .bind(&task_id.0)
        .bind(agent_id.map(Id::as_str))
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(super::sqlx_err)?
        .rows_affected();

        if rows == 0 {
            return Err(Error::not_found("leased Probe task"));
        }

        Ok(())
    }

    async fn expire_leases(&self, now: TimestampMicros, limit: u32) -> Result<u64> {
        let result = sqlx::query(
            "WITH expired AS (
                 SELECT id FROM synthetic_probe_tasks
                 WHERE state IN ('leased', 'running') AND leased_until <= $1
                 ORDER BY leased_until, id
                 LIMIT $2 FOR UPDATE SKIP LOCKED
             )
             UPDATE synthetic_probe_tasks task
             SET state = CASE WHEN deadline_at > $1 THEN 'pending' ELSE 'expired' END,
                 lease_agent_id = NULL, lease_token_hash = NULL, leased_until = NULL,
                 updated_at = $1
             FROM expired WHERE task.id = expired.id",
        )
        .bind(now)
        .bind(i64::from(limit.min(10_000)))
        .execute(&self.pool)
        .await
        .map_err(super::sqlx_err)?;

        Ok(result.rows_affected())
    }

    async fn expire_unclaimed(&self, now: TimestampMicros, limit: u32) -> Result<Vec<ProbeTask>> {
        // A task no Agent took by its deadline would otherwise stay pending for good, and every
        // later run of its check at that location would be skipped behind it.
        let sql = format!(
            "WITH unclaimed AS (
                 SELECT id AS task_id FROM synthetic_probe_tasks
                 WHERE state = 'pending' AND deadline_at <= $1
                 ORDER BY deadline_at, id
                 LIMIT $2 FOR UPDATE SKIP LOCKED
             )
             UPDATE synthetic_probe_tasks task
             SET state = 'expired', updated_at = $1
             FROM unclaimed WHERE task.id = unclaimed.task_id
             RETURNING {TASK_COLS}"
        );

        sqlx::query(&sql)
            .bind(now)
            .bind(i64::from(limit.min(10_000)))
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(row_to_task)
            .collect()
    }

    async fn get_leased_task(
        &self,
        task_id: &Id,
        agent_id: &Id,
        lease_token_hash: &str,
        now: TimestampMicros,
    ) -> Result<ProbeTask> {
        let sql = format!(
            "SELECT {TASK_COLS} FROM synthetic_probe_tasks
             WHERE id = $1 AND lease_agent_id = $2 AND lease_token_hash = $3
               AND ((state IN ('leased', 'running') AND leased_until > $4)
                    OR state = 'completed')"
        );

        let row = sqlx::query(&sql)
            .bind(&task_id.0)
            .bind(&agent_id.0)
            .bind(lease_token_hash)
            .bind(now)
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?;

        row_to_task(row)
    }

    async fn get_leased_task_by_token(
        &self,
        task_id: &Id,
        lease_token_hash: &str,
        now: TimestampMicros,
    ) -> Result<ProbeTask> {
        let sql = format!(
            "SELECT {TASK_COLS} FROM synthetic_probe_tasks
             WHERE id = $1 AND lease_token_hash = $2
               AND state IN ('leased', 'running') AND leased_until > $3"
        );

        let row = sqlx::query(&sql)
            .bind(&task_id.0)
            .bind(lease_token_hash)
            .bind(now)
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?;

        row_to_task(row)
    }

    async fn acknowledge_task(
        &self,
        task_id: &Id,
        agent_id: &Id,
        lease_token_hash: &str,
        accepted: bool,
        now: TimestampMicros,
    ) -> Result<()> {
        let rows = sqlx::query(
            "UPDATE synthetic_probe_tasks
             SET state = CASE WHEN $4 THEN 'running'
                              WHEN deadline_at > $5 THEN 'pending'
                              ELSE 'expired' END,
                 lease_agent_id = CASE WHEN $4 THEN lease_agent_id ELSE NULL END,
                 lease_token_hash = CASE WHEN $4 THEN lease_token_hash ELSE NULL END,
                 leased_until = CASE WHEN $4 THEN leased_until ELSE NULL END,
                 updated_at = $5
             WHERE id = $1 AND lease_agent_id = $2 AND lease_token_hash = $3
               AND state = 'leased'",
        )
        .bind(&task_id.0)
        .bind(&agent_id.0)
        .bind(lease_token_hash)
        .bind(accepted)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(super::sqlx_err)?
        .rows_affected();

        if rows == 0 {
            return Err(Error::conflict(
                "Probe task acknowledgement no longer owns its lease",
            ));
        }

        Ok(())
    }

    async fn renew_task_lease(
        &self,
        task_id: &Id,
        agent_id: &Id,
        lease_token_hash: &str,
        requested_until: TimestampMicros,
        now: TimestampMicros,
    ) -> Result<TimestampMicros> {
        let maximum = TimestampMicros(now.0.saturating_add(120 * 1_000_000));
        let requested = TimestampMicros(requested_until.0.max(now.0).min(maximum.0));

        let value: Option<TimestampMicros> = sqlx::query_scalar(
            "UPDATE synthetic_probe_tasks
             SET leased_until = LEAST(deadline_at, $5),
                 updated_at = $4
             WHERE id = $1 AND lease_agent_id = $2 AND lease_token_hash = $3
               AND state IN ('leased', 'running') AND leased_until > $4
             RETURNING leased_until",
        )
        .bind(&task_id.0)
        .bind(&agent_id.0)
        .bind(lease_token_hash)
        .bind(now)
        .bind(requested)
        .fetch_optional(&self.pool)
        .await
        .map_err(super::sqlx_err)?;

        value.ok_or_else(|| Error::conflict("Probe task lease cannot be renewed"))
    }
}
