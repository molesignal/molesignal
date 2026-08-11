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
                     is_test, state, scheduled_at_micros, deadline_at_micros, timeout_millis,
                     max_attempts, lease_agent_id, lease_token_hash, leased_until_micros,
                     created_at_micros, updated_at_micros)
                 VALUES ($1, $2, $3, $4, $5, $6, $7,
                         CASE WHEN $7 OR NOT EXISTS (
                             SELECT 1 FROM synthetic_probe_tasks existing
                             WHERE existing.organization_id = $2
                               AND existing.monitor_id = $3 AND existing.location_id = $5
                               AND NOT existing.is_test
                               AND existing.state IN ('pending', 'leased', 'running')
                         ) THEN 'pending' ELSE 'skipped' END,
                         $8, $9, $10, $11,
                         NULL, NULL, NULL, $12, $13)
                 ON CONFLICT (monitor_revision_id, location_id, scheduled_at_micros, is_test)
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
                .bind(task.scheduled_at.0)
                .bind(task.deadline_at.0)
                .bind(task.timeout_millis as i32)
                .bind(task.max_attempts as i16)
                .bind(task.created_at.0)
                .bind(task.updated_at.0)
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
                   AND task.scheduled_at_micros <= $3 AND task.deadline_at_micros > $3
                   AND location.organization_id IS NOT DISTINCT FROM $2
                   AND revision.spec->>'kind' = ANY($7)
                 ORDER BY task.scheduled_at_micros, task.id
                 LIMIT 1 FOR UPDATE OF task SKIP LOCKED
             )
             UPDATE synthetic_probe_tasks task
             SET state = 'leased', lease_agent_id = $4, lease_token_hash = $5,
                 leased_until_micros = $6, updated_at_micros = $3
             FROM candidate
             WHERE task.id = candidate.task_id
             RETURNING {TASK_COLS}"
        );
        sqlx::query(&sql)
            .bind(&agent.location_id.0)
            .bind(agent.organization_id.as_ref().map(Id::as_str))
            .bind(now.0)
            .bind(&agent.id.0)
            .bind(lease_token_hash)
            .bind(leased_until.0)
            .bind(
                agent
                    .capabilities
                    .iter()
                    .map(|capability| capability.as_str().to_string())
                    .collect::<Vec<_>>(),
            )
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
             SET state = 'completed', updated_at_micros = $4
             WHERE organization_id = $1 AND id = $2
               AND lease_agent_id IS NOT DISTINCT FROM $3
               AND state IN ('leased', 'running', 'completed')",
        )
        .bind(&org_id.0)
        .bind(&task_id.0)
        .bind(agent_id.map(Id::as_str))
        .bind(now.0)
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
                 WHERE state IN ('leased', 'running') AND leased_until_micros <= $1
                 ORDER BY leased_until_micros, id
                 LIMIT $2 FOR UPDATE SKIP LOCKED
             )
             UPDATE synthetic_probe_tasks task
             SET state = CASE WHEN deadline_at_micros > $1 THEN 'pending' ELSE 'expired' END,
                 lease_agent_id = NULL, lease_token_hash = NULL, leased_until_micros = NULL,
                 updated_at_micros = $1
             FROM expired WHERE task.id = expired.id",
        )
        .bind(now.0)
        .bind(i64::from(limit.min(10_000)))
        .execute(&self.pool)
        .await
        .map_err(super::sqlx_err)?;
        Ok(result.rows_affected())
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
               AND ((state IN ('leased', 'running') AND leased_until_micros > $4)
                    OR state = 'completed')"
        );
        let row = sqlx::query(&sql)
            .bind(&task_id.0)
            .bind(&agent_id.0)
            .bind(lease_token_hash)
            .bind(now.0)
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
                              WHEN deadline_at_micros > $5 THEN 'pending'
                              ELSE 'expired' END,
                 lease_agent_id = CASE WHEN $4 THEN lease_agent_id ELSE NULL END,
                 lease_token_hash = CASE WHEN $4 THEN lease_token_hash ELSE NULL END,
                 leased_until_micros = CASE WHEN $4 THEN leased_until_micros ELSE NULL END,
                 updated_at_micros = $5
             WHERE id = $1 AND lease_agent_id = $2 AND lease_token_hash = $3
               AND state = 'leased'",
        )
        .bind(&task_id.0)
        .bind(&agent_id.0)
        .bind(lease_token_hash)
        .bind(accepted)
        .bind(now.0)
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
        let requested = requested_until.0.max(now.0).min(maximum.0);
        let value: Option<i64> = sqlx::query_scalar(
            "UPDATE synthetic_probe_tasks
             SET leased_until_micros = LEAST(deadline_at_micros, $5),
                 updated_at_micros = $4
             WHERE id = $1 AND lease_agent_id = $2 AND lease_token_hash = $3
               AND state IN ('leased', 'running') AND leased_until_micros > $4
             RETURNING leased_until_micros",
        )
        .bind(&task_id.0)
        .bind(&agent_id.0)
        .bind(lease_token_hash)
        .bind(now.0)
        .bind(requested)
        .fetch_optional(&self.pool)
        .await
        .map_err(super::sqlx_err)?;
        value
            .map(TimestampMicros)
            .ok_or_else(|| Error::conflict("Probe task lease cannot be renewed"))
    }
}
