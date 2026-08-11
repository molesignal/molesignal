// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;

use super::{
    PgSyntheticRepository,
    codec::{MONITOR_COLS, REVISION_COLS, row_to_monitor, row_to_revision},
};
use crate::{
    domain::synthetics::{
        ActiveMonitorRevision, MonitorLifecycle, MonitorRevision, MonitorSpec, MonitorState,
        SyntheticMonitor, SyntheticMonitorRepository,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const REVISION_CAN_PUBLISH_SQL: &str = "SELECT EXISTS(
        SELECT 1 FROM synthetic_monitor_revisions
        WHERE organization_id = $1 AND monitor_id = $2 AND id = $3
          AND (spec->>'kind' = 'heartbeat' OR last_test_passed_at_micros IS NOT NULL)
     )";

async fn insert_revision(
    connection: &mut sqlx::PgConnection,
    revision: &MonitorRevision,
) -> Result<()> {
    let kind: String = sqlx::query_scalar(
        "SELECT kind FROM synthetic_monitors
         WHERE organization_id = $1 AND id = $2 AND lifecycle <> 'archived'
         FOR UPDATE",
    )
    .bind(&revision.organization_id.0)
    .bind(&revision.monitor_id.0)
    .fetch_one(&mut *connection)
    .await
    .map_err(super::sqlx_err)?;
    if kind != revision.spec.kind().as_str() {
        return Err(Error::invalid("Monitor Revision type cannot change"));
    }
    let expected_number: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(revision_number), 0) + 1
         FROM synthetic_monitor_revisions
         WHERE organization_id = $1 AND monitor_id = $2",
    )
    .bind(&revision.organization_id.0)
    .bind(&revision.monitor_id.0)
    .fetch_one(&mut *connection)
    .await
    .map_err(super::sqlx_err)?;
    if revision.number != expected_number as u32 {
        return Err(Error::conflict(format!(
            "next Monitor Revision is {expected_number}, received {}",
            revision.number
        )));
    }
    validate_locations(connection, revision).await?;
    sqlx::query(
        "INSERT INTO synthetic_monitor_revisions
            (id, organization_id, monitor_id, revision_number, spec, schedule,
             timeout_millis, max_retries, consecutive_failures, consecutive_recoveries,
             freshness_seconds, location_policy, escalation_policy_id, alert_on_degraded,
             last_test_result_id, last_test_passed_at_micros,
             created_by, created_at_micros, content_hash)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                 $15, $16, $17, $18, $19)",
    )
    .bind(&revision.id.0)
    .bind(&revision.organization_id.0)
    .bind(&revision.monitor_id.0)
    .bind(revision.number as i32)
    .bind(sqlx::types::Json(&revision.spec))
    .bind(sqlx::types::Json(&revision.schedule))
    .bind(revision.timeout_millis as i32)
    .bind(revision.max_retries as i16)
    .bind(revision.consecutive_failures as i32)
    .bind(revision.consecutive_recoveries as i32)
    .bind(revision.freshness_seconds as i32)
    .bind(sqlx::types::Json(&revision.location_policy))
    .bind(revision.escalation_policy_id.as_ref().map(Id::as_str))
    .bind(revision.alert_on_degraded)
    .bind(revision.last_test_result_id.as_ref().map(Id::as_str))
    .bind(revision.last_test_passed_at.map(|value| value.0))
    .bind(&revision.created_by.0)
    .bind(revision.created_at.0)
    .bind(&revision.content_hash)
    .execute(&mut *connection)
    .await
    .map_err(super::sqlx_err)?;
    for (position, location_id) in revision.location_ids.iter().enumerate() {
        sqlx::query(
            "INSERT INTO synthetic_monitor_revision_locations
                (organization_id, revision_id, location_id, position)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(&revision.organization_id.0)
        .bind(&revision.id.0)
        .bind(&location_id.0)
        .bind(position as i32)
        .execute(&mut *connection)
        .await
        .map_err(super::sqlx_err)?;
    }
    Ok(())
}

async fn validate_locations(
    connection: &mut sqlx::PgConnection,
    revision: &MonitorRevision,
) -> Result<()> {
    if matches!(revision.spec, MonitorSpec::Heartbeat) {
        if !revision.location_ids.is_empty() {
            return Err(Error::invalid("Heartbeat Monitors do not use Locations"));
        }
        return Ok(());
    }
    if revision.location_ids.is_empty() {
        return Err(Error::invalid(
            "active probe Monitors require at least one Location",
        ));
    }
    let ids = revision
        .location_ids
        .iter()
        .map(Id::as_str)
        .collect::<Vec<_>>();
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM synthetic_probe_locations
         WHERE id = ANY($2) AND lifecycle = 'active'
           AND (organization_id = $1 OR organization_id IS NULL)",
    )
    .bind(&revision.organization_id.0)
    .bind(&ids)
    .fetch_one(&mut *connection)
    .await
    .map_err(super::sqlx_err)?;
    if count != ids.len() as i64 {
        return Err(Error::invalid(
            "Monitor Revision contains unavailable or cross-organization Locations",
        ));
    }
    Ok(())
}

async fn load_revision(
    pool: &sqlx::PgPool,
    org_id: &Id,
    monitor_id: &Id,
    revision_id: &Id,
) -> Result<MonitorRevision> {
    let sql = format!(
        "SELECT {REVISION_COLS} FROM synthetic_monitor_revisions
         WHERE organization_id = $1 AND monitor_id = $2 AND id = $3"
    );
    let row = sqlx::query(&sql)
        .bind(&org_id.0)
        .bind(&monitor_id.0)
        .bind(&revision_id.0)
        .fetch_one(pool)
        .await
        .map_err(super::sqlx_err)?;
    let location_ids = sqlx::query_scalar::<String>(
        "SELECT location_id FROM synthetic_monitor_revision_locations
         WHERE organization_id = $1 AND revision_id = $2
         ORDER BY position, location_id",
    )
    .bind(&org_id.0)
    .bind(&revision_id.0)
    .fetch_all(pool)
    .await
    .map_err(super::sqlx_err)?
    .into_iter()
    .map(Id)
    .collect();
    row_to_revision(row, location_ids)
}

#[async_trait]
impl SyntheticMonitorRepository for PgSyntheticRepository {
    async fn create_monitor(
        &self,
        monitor: SyntheticMonitor,
        revision: MonitorRevision,
    ) -> Result<ActiveMonitorRevision> {
        if monitor.id != revision.monitor_id
            || monitor.organization_id != revision.organization_id
            || monitor.kind != revision.spec.kind()
            || revision.number != 1
        {
            return Err(Error::invalid("Monitor and first Revision do not match"));
        }
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        let sql = format!(
            "INSERT INTO synthetic_monitors
                (id, organization_id, name, description, kind, lifecycle, state, team_id,
                 tags, active_revision_id, draft_revision_id, next_due_at_micros, created_by,
                 created_at_micros, updated_at_micros, archived_at_micros)
             VALUES ($1, $2, $3, $4, $5, 'draft', $6, $7, $8, NULL, NULL, NULL,
                     $9, $10, $11, NULL)
             RETURNING {MONITOR_COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(&monitor.id.0)
            .bind(&monitor.organization_id.0)
            .bind(&monitor.name)
            .bind(&monitor.description)
            .bind(monitor.kind.as_str())
            .bind(monitor.state.as_str())
            .bind(monitor.team_id.as_ref().map(Id::as_str))
            .bind(&monitor.tags)
            .bind(&monitor.created_by.0)
            .bind(monitor.created_at.0)
            .bind(monitor.updated_at.0)
            .fetch_one(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?;
        insert_revision(&mut transaction, &revision).await?;
        sqlx::query(
            "UPDATE synthetic_monitors SET draft_revision_id = $3
             WHERE organization_id = $1 AND id = $2",
        )
        .bind(&monitor.organization_id.0)
        .bind(&monitor.id.0)
        .bind(&revision.id.0)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        let mut monitor = row_to_monitor(row)?;
        monitor.draft_revision_id = Some(revision.id.clone());
        transaction.commit().await.map_err(super::sqlx_err)?;
        Ok(ActiveMonitorRevision { monitor, revision })
    }

    async fn create_draft_revision(&self, revision: MonitorRevision) -> Result<MonitorRevision> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        insert_revision(&mut transaction, &revision).await?;
        sqlx::query(
            "UPDATE synthetic_monitors
             SET draft_revision_id = $3, updated_at_micros = $4
             WHERE organization_id = $1 AND id = $2 AND lifecycle <> 'archived'",
        )
        .bind(&revision.organization_id.0)
        .bind(&revision.monitor_id.0)
        .bind(&revision.id.0)
        .bind(revision.created_at.0)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;
        transaction.commit().await.map_err(super::sqlx_err)?;
        Ok(revision)
    }

    async fn get_revision(
        &self,
        org_id: &Id,
        monitor_id: &Id,
        revision_id: &Id,
    ) -> Result<MonitorRevision> {
        load_revision(&self.pool, org_id, monitor_id, revision_id).await
    }

    async fn list_revisions(&self, org_id: &Id, monitor_id: &Id) -> Result<Vec<MonitorRevision>> {
        let ids = sqlx::query_scalar::<String>(
            "SELECT id FROM synthetic_monitor_revisions
             WHERE organization_id = $1 AND monitor_id = $2
             ORDER BY revision_number DESC",
        )
        .bind(&org_id.0)
        .bind(&monitor_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(super::sqlx_err)?;
        let mut revisions = Vec::with_capacity(ids.len());
        for id in ids {
            revisions.push(load_revision(&self.pool, org_id, monitor_id, &Id(id)).await?);
        }
        Ok(revisions)
    }

    async fn activate_revision(
        &self,
        org_id: &Id,
        monitor_id: &Id,
        revision_id: &Id,
        next_due_at: Option<TimestampMicros>,
        updated_at: TimestampMicros,
    ) -> Result<ActiveMonitorRevision> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        let can_publish: bool = sqlx::query_scalar(REVISION_CAN_PUBLISH_SQL)
            .bind(&org_id.0)
            .bind(&monitor_id.0)
            .bind(&revision_id.0)
            .fetch_one(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?;
        if !can_publish {
            return Err(Error::conflict(
                "Monitor Revision must pass a Test Run before publishing",
            ));
        }
        let sql = format!(
            "UPDATE synthetic_monitors
             SET active_revision_id = $3,
                 draft_revision_id = CASE WHEN draft_revision_id = $3 THEN NULL ELSE draft_revision_id END,
                 lifecycle = 'active', next_due_at_micros = $4,
                 archived_at_micros = NULL, updated_at_micros = $5
             WHERE organization_id = $1 AND id = $2 AND lifecycle <> 'archived'
             RETURNING {MONITOR_COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&monitor_id.0)
            .bind(&revision_id.0)
            .bind(next_due_at.map(|value| value.0))
            .bind(updated_at.0)
            .fetch_one(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?;
        let monitor = row_to_monitor(row)?;
        transaction.commit().await.map_err(super::sqlx_err)?;
        let revision = load_revision(&self.pool, org_id, monitor_id, revision_id).await?;
        Ok(ActiveMonitorRevision { monitor, revision })
    }

    async fn mark_revision_tested(
        &self,
        org_id: &Id,
        monitor_id: &Id,
        revision_id: &Id,
        result_id: &Id,
        passed_at: TimestampMicros,
    ) -> Result<MonitorRevision> {
        let rows = sqlx::query(
            "UPDATE synthetic_monitor_revisions revision
             SET last_test_result_id = $4, last_test_passed_at_micros = $5
             FROM synthetic_results result
             WHERE revision.organization_id = $1 AND revision.monitor_id = $2
               AND revision.id = $3 AND result.organization_id = $1
               AND result.id = $4 AND result.monitor_revision_id = revision.id
               AND result.is_test AND result.outcome IN ('healthy', 'degraded')",
        )
        .bind(&org_id.0)
        .bind(&monitor_id.0)
        .bind(&revision_id.0)
        .bind(&result_id.0)
        .bind(passed_at.0)
        .execute(&self.pool)
        .await
        .map_err(super::sqlx_err)?
        .rows_affected();
        if rows == 0 {
            return Err(Error::conflict(
                "Test Run did not pass this Monitor Revision",
            ));
        }
        load_revision(&self.pool, org_id, monitor_id, revision_id).await
    }

    async fn update_monitor_runtime(
        &self,
        org_id: &Id,
        monitor_id: &Id,
        lifecycle: MonitorLifecycle,
        state: MonitorState,
        next_due_at: Option<TimestampMicros>,
        updated_at: TimestampMicros,
    ) -> Result<SyntheticMonitor> {
        let sql = format!(
            "UPDATE synthetic_monitors
             SET lifecycle = $3, state = $4, next_due_at_micros = $5,
                 archived_at_micros = CASE WHEN $3 = 'archived' THEN $6 ELSE NULL END,
                 updated_at_micros = $6
             WHERE organization_id = $1 AND id = $2
             RETURNING {MONITOR_COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&monitor_id.0)
            .bind(lifecycle.as_str())
            .bind(state.as_str())
            .bind(next_due_at.map(|value| value.0))
            .bind(updated_at.0)
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?;
        row_to_monitor(row)
    }

    async fn get_monitor(&self, org_id: &Id, monitor_id: &Id) -> Result<SyntheticMonitor> {
        let sql = format!(
            "SELECT {MONITOR_COLS} FROM synthetic_monitors
             WHERE organization_id = $1 AND id = $2"
        );
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&monitor_id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?;
        row_to_monitor(row)
    }

    async fn get_active_revision(
        &self,
        org_id: &Id,
        monitor_id: &Id,
    ) -> Result<Option<ActiveMonitorRevision>> {
        let monitor = self.get_monitor(org_id, monitor_id).await?;
        let Some(revision_id) = &monitor.active_revision_id else {
            return Ok(None);
        };
        let revision = load_revision(&self.pool, org_id, monitor_id, revision_id).await?;
        Ok(Some(ActiveMonitorRevision { monitor, revision }))
    }

    async fn list_monitors(&self, org_id: &Id) -> Result<Vec<SyntheticMonitor>> {
        let sql = format!(
            "SELECT {MONITOR_COLS} FROM synthetic_monitors
             WHERE organization_id = $1
             ORDER BY archived_at_micros NULLS FIRST, updated_at_micros DESC, id DESC"
        );
        sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(row_to_monitor)
            .collect()
    }

    async fn list_due_monitors(
        &self,
        due_at: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<ActiveMonitorRevision>> {
        let sql = format!(
            "SELECT {MONITOR_COLS} FROM synthetic_monitors
             WHERE lifecycle = 'active' AND active_revision_id IS NOT NULL
               AND next_due_at_micros <= $1
             ORDER BY next_due_at_micros, id LIMIT $2"
        );
        let monitors = sqlx::query(&sql)
            .bind(due_at.0)
            .bind(i64::from(limit.min(1000)))
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(row_to_monitor)
            .collect::<Result<Vec<_>>>()?;
        let mut due = Vec::with_capacity(monitors.len());
        for monitor in monitors {
            let revision_id = monitor
                .active_revision_id
                .as_ref()
                .ok_or_else(|| Error::internal("due Monitor has no active Revision"))?;
            let revision = load_revision(
                &self.pool,
                &monitor.organization_id,
                &monitor.id,
                revision_id,
            )
            .await?;
            due.push(ActiveMonitorRevision { monitor, revision });
        }
        Ok(due)
    }
}

#[cfg(test)]
mod tests {
    use super::REVISION_CAN_PUBLISH_SQL;

    #[test]
    fn revision_test_gate_keeps_postgres_boolean_result() {
        let normalized = REVISION_CAN_PUBLISH_SQL.to_ascii_lowercase();
        assert!(normalized.starts_with("select exists("));
        assert!(!normalized.contains("::int"));
        assert!(!normalized.contains("bigint"));
        assert!(normalized.contains("spec->>'kind' = 'heartbeat'"));
    }
}
