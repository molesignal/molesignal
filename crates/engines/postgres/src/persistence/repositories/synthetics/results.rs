// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashMap;

use async_trait::async_trait;
use sqlx::{PgConnection, Row};

use super::{
    PgSyntheticRepository,
    codec::{RESULT_COLS, row_to_result},
    retention,
};
use crate::{
    domain::synthetics::{
        PurgedResults, SyntheticResult, SyntheticResultArtifact, SyntheticResultListQuery,
        SyntheticResultPage, SyntheticResultRepository,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const RESULT_PAGE_FILTERS: &str = "organization_id = $1
    AND ($2::TEXT IS NULL OR EXISTS (
        SELECT 1 FROM synthetic_monitors monitor
        WHERE monitor.organization_id = synthetic_results.organization_id
          AND monitor.id = synthetic_results.monitor_id
          AND (POSITION(LOWER($2::TEXT) IN LOWER(monitor.name)) > 0
               OR POSITION(LOWER($2::TEXT) IN LOWER(monitor.id)) > 0)
    ))
    AND ($3::TEXT IS NULL OR outcome = $3)
    AND ($4::TEXT IS NULL OR location_id = $4)
    AND ($5::TIMESTAMPTZ IS NULL OR finished_at >= $5)
    AND ($6::TIMESTAMPTZ IS NULL OR finished_at < $6)";

fn insert_result_sql() -> String {
    format!(
        "INSERT INTO synthetic_results
            (id, organization_id, monitor_id, monitor_revision_id, location_id, agent_id,
             task_id, is_test, result_sequence, scheduled_at, started_at,
             finished_at, received_at, outcome, attempts, assertions,
             secret_versions, protocol_version, metadata)
         SELECT $1, $2, $3, $4, $5, $6, $7, task.is_test, $8, $9, $10, $11, $12,
                $13, $14, $15, $16, $17, $18
         FROM synthetic_probe_tasks task
         WHERE task.id = $7 AND task.organization_id = $2 AND task.monitor_id = $3
           AND task.monitor_revision_id = $4 AND task.location_id = $5
           AND task.lease_agent_id IS NOT DISTINCT FROM $6
           AND (task.state IN ('leased', 'running', 'completed')
                -- The server's own record of a run no Agent took before its deadline.
                OR (task.state = 'expired' AND $6::VARCHAR IS NULL AND $13 = 'skipped'))
         ON CONFLICT (task_id) DO UPDATE SET id = synthetic_results.id
         RETURNING {RESULT_COLS}, (xmax = 0) AS inserted"
    )
}

const UPDATE_AGENT_RESULT_SEQUENCE_SQL: &str = "UPDATE synthetic_probe_agents agent
     SET last_result_sequence = GREATEST(agent.last_result_sequence, $3),
         updated_at = GREATEST(agent.updated_at, $4)
     FROM synthetic_probe_locations location
     WHERE agent.id = $2 AND agent.location_id = location.id
       AND (location.organization_id = $1 OR location.organization_id IS NULL)";

#[async_trait]
impl SyntheticResultRepository for PgSyntheticRepository {
    async fn record_result(&self, result: SyntheticResult) -> Result<(SyntheticResult, bool)> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        let sql = insert_result_sql();

        let row = sqlx::query(&sql)
            .bind(&result.id.0)
            .bind(&result.organization_id.0)
            .bind(&result.monitor_id.0)
            .bind(&result.monitor_revision_id.0)
            .bind(&result.location_id.0)
            .bind(result.agent_id.as_ref().map(Id::as_str))
            .bind(&result.task_id.0)
            .bind(result.result_sequence.map(|value| value as i64))
            .bind(result.scheduled_at)
            .bind(result.started_at)
            .bind(result.finished_at)
            .bind(result.received_at)
            .bind(result.outcome.as_str())
            .bind(sqlx::types::Json(&result.attempts))
            .bind(sqlx::types::Json(&result.assertions))
            .bind(sqlx::types::Json(&result.secret_versions))
            .bind(result.protocol_version as i32)
            .bind(sqlx::types::Json(&result.metadata))
            .fetch_optional(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?;

        let Some(row) = row else {
            return Err(Error::conflict(
                "Probe result does not match its organization-scoped task lease",
            ));
        };

        // A skipped run stays an expired task: no Agent completed it.
        sqlx::query(
            "UPDATE synthetic_probe_tasks
             SET state = 'completed', updated_at = $3
             WHERE organization_id = $1 AND id = $2 AND state <> 'expired'",
        )
        .bind(&result.organization_id.0)
        .bind(&result.task_id.0)
        .bind(result.received_at)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?;

        if let (Some(agent_id), Some(sequence)) = (&result.agent_id, result.result_sequence) {
            sqlx::query(UPDATE_AGENT_RESULT_SEQUENCE_SQL)
                .bind(result.organization_id.as_str())
                .bind(&agent_id.0)
                .bind(sequence as i64)
                .bind(result.received_at)
                .execute(&mut *transaction)
                .await
                .map_err(super::sqlx_err)?;
        }

        let inserted: bool = row.try_get("inserted").map_err(super::sqlx_err)?;

        if inserted {
            insert_artifacts(&mut transaction, &result).await?;
        }

        let mut persisted = row_to_result(row)?;

        persisted.artifacts =
            load_artifacts_in_transaction(&mut transaction, &result.organization_id, &persisted.id)
                .await?;

        transaction.commit().await.map_err(super::sqlx_err)?;
        Ok((persisted, inserted))
    }

    async fn list_results(
        &self,
        org_id: &Id,
        monitor_id: &Id,
        before: Option<TimestampMicros>,
        limit: u32,
    ) -> Result<Vec<SyntheticResult>> {
        let sql = format!(
            "SELECT {RESULT_COLS} FROM synthetic_results
             WHERE organization_id = $1 AND monitor_id = $2
               AND ($3::TIMESTAMPTZ IS NULL OR finished_at < $3)
             ORDER BY finished_at DESC, id DESC LIMIT $4"
        );

        let mut results = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&monitor_id.0)
            .bind(before)
            .bind(i64::from(limit.clamp(1, 500)))
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(row_to_result)
            .collect::<Result<Vec<_>>>()?;

        attach_artifacts(&self.pool, org_id, &mut results).await?;
        Ok(results)
    }

    async fn list_results_page(
        &self,
        org_id: &Id,
        query: &SyntheticResultListQuery,
    ) -> Result<SyntheticResultPage> {
        let count_sql =
            format!("SELECT COUNT(*) AS total FROM synthetic_results WHERE {RESULT_PAGE_FILTERS}");

        let total: i64 = sqlx::query(&count_sql)
            .bind(org_id.as_str())
            .bind(query.check_query.as_deref())
            .bind(query.outcome.map(|outcome| outcome.as_str()))
            .bind(query.location_id.as_ref().map(Id::as_str))
            .bind(query.from)
            .bind(query.to)
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .try_get("total")
            .map_err(super::sqlx_err)?;

        let page_sql = format!(
            "SELECT {RESULT_COLS} FROM synthetic_results
             WHERE {RESULT_PAGE_FILTERS}
             ORDER BY finished_at DESC, id DESC LIMIT $7 OFFSET $8"
        );

        let limit = i64::from(query.limit.clamp(1, 500));
        let offset = i64::try_from(query.offset).unwrap_or(i64::MAX);

        let mut rows = sqlx::query(&page_sql)
            .bind(org_id.as_str())
            .bind(query.check_query.as_deref())
            .bind(query.outcome.map(|outcome| outcome.as_str()))
            .bind(query.location_id.as_ref().map(Id::as_str))
            .bind(query.from)
            .bind(query.to)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(row_to_result)
            .collect::<Result<Vec<_>>>()?;

        attach_artifacts(&self.pool, org_id, &mut rows).await?;

        Ok(SyntheticResultPage {
            items: rows,
            total: u64::try_from(total)
                .map_err(|_| Error::internal("synthetic result count cannot be negative"))?,
        })
    }

    async fn get_result(&self, org_id: &Id, result_id: &Id) -> Result<SyntheticResult> {
        let sql = format!(
            "SELECT {RESULT_COLS} FROM synthetic_results
             WHERE organization_id = $1 AND id = $2"
        );

        let row = sqlx::query(&sql)
            .bind(org_id.as_str())
            .bind(result_id.as_str())
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?;

        let mut result = row_to_result(row)?;
        attach_artifacts(&self.pool, org_id, std::slice::from_mut(&mut result)).await?;
        Ok(result)
    }

    async fn purge_results(&self, before: TimestampMicros, limit: u32) -> Result<PurgedResults> {
        retention::purge_results(self, before, limit).await
    }

    async fn purge_expired_artifacts(
        &self,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<String>> {
        retention::purge_expired_artifacts(self, now, limit).await
    }

    async fn purge_finished_tasks(&self, before: TimestampMicros, limit: u32) -> Result<u64> {
        retention::purge_finished_tasks(self, before, limit).await
    }
}

async fn insert_artifacts(connection: &mut PgConnection, result: &SyntheticResult) -> Result<()> {
    for artifact in &result.artifacts {
        let sha256 = hex::decode(&artifact.sha256)
            .map_err(|_| Error::invalid("invalid synthetic Artifact SHA-256"))?;
        let content_length = i64::try_from(artifact.content_length)
            .map_err(|_| Error::invalid("synthetic Artifact is too large"))?;

        let inserted = sqlx::query(
            "INSERT INTO synthetic_result_artifacts
                (id, organization_id, result_id, kind, name, object_key, content_type,
                 content_length, sha256, expires_at, created_at)
             SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11
             FROM synthetic_results result
             WHERE result.id = $3 AND result.organization_id = $2",
        )
        .bind(artifact.id.as_str())
        .bind(result.organization_id.as_str())
        .bind(result.id.as_str())
        .bind(&artifact.kind)
        .bind(&artifact.name)
        .bind(&artifact.object_key)
        .bind(&artifact.content_type)
        .bind(content_length)
        .bind(sha256)
        .bind(artifact.expires_at)
        .bind(artifact.created_at)
        .execute(&mut *connection)
        .await
        .map_err(super::sqlx_err)?
        .rows_affected();

        if inserted != 1 {
            return Err(Error::conflict(
                "synthetic Artifact does not match its organization-scoped result",
            ));
        }
    }

    Ok(())
}

async fn load_artifacts_in_transaction(
    connection: &mut PgConnection,
    org_id: &Id,
    result_id: &Id,
) -> Result<Vec<SyntheticResultArtifact>> {
    sqlx::query(
        "SELECT id, result_id, kind, name, object_key, content_type, content_length, sha256,
                expires_at, created_at
         FROM synthetic_result_artifacts
         WHERE organization_id = $1 AND result_id = $2
         ORDER BY created_at, id",
    )
    .bind(org_id.as_str())
    .bind(result_id.as_str())
    .fetch_all(&mut *connection)
    .await
    .map_err(super::sqlx_err)?
    .into_iter()
    .map(row_to_artifact)
    .collect()
}

async fn attach_artifacts(
    pool: &sqlx::PgPool,
    org_id: &Id,
    results: &mut [SyntheticResult],
) -> Result<()> {
    if results.is_empty() {
        return Ok(());
    }

    let result_ids = results
        .iter()
        .map(|result| result.id.as_str().to_string())
        .collect::<Vec<_>>();

    let rows = sqlx::query(
        "SELECT id, result_id, kind, name, object_key, content_type, content_length, sha256,
                expires_at, created_at
         FROM synthetic_result_artifacts
         WHERE organization_id = $1 AND result_id = ANY($2::TEXT[])
         ORDER BY created_at, id",
    )
    .bind(org_id.as_str())
    .bind(&result_ids)
    .fetch_all(pool)
    .await
    .map_err(super::sqlx_err)?;

    let mut by_result: HashMap<String, Vec<SyntheticResultArtifact>> = HashMap::new();

    for row in rows {
        let result_id: String = row.try_get("result_id").map_err(super::sqlx_err)?;

        by_result
            .entry(result_id)
            .or_default()
            .push(row_to_artifact(row)?);
    }

    for result in results {
        result.artifacts = by_result.remove(result.id.as_str()).unwrap_or_default();
    }

    Ok(())
}

fn row_to_artifact(row: sqlx::postgres::PgRow) -> Result<SyntheticResultArtifact> {
    let content_length = u64::try_from(
        row.try_get::<i64, _>("content_length")
            .map_err(super::sqlx_err)?,
    )
    .map_err(|_| Error::internal("negative synthetic Artifact length"))?;

    let sha256: Vec<u8> = row.try_get("sha256").map_err(super::sqlx_err)?;

    Ok(SyntheticResultArtifact {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(super::sqlx_err)?),
        name: row.try_get("name").map_err(super::sqlx_err)?,
        kind: row.try_get("kind").map_err(super::sqlx_err)?,
        object_key: row.try_get("object_key").map_err(super::sqlx_err)?,
        content_type: row.try_get("content_type").map_err(super::sqlx_err)?,
        content_length,
        sha256: hex::encode(sha256),
        expires_at: row
            .try_get::<TimestampMicros, _>("expires_at")
            .map_err(super::sqlx_err)?,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(super::sqlx_err)?,
    })
}

#[cfg(test)]
mod tests {
    use super::{RESULT_PAGE_FILTERS, UPDATE_AGENT_RESULT_SEQUENCE_SQL, insert_result_sql};

    #[test]
    fn result_test_flag_comes_from_the_leased_task() {
        let normalized = insert_result_sql()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");

        assert!(normalized.contains("$7, task.is_test, $8, $9"));
        assert!(!normalized.contains("$7, $8, $9, $10"));
    }

    #[test]
    fn agent_result_update_qualifies_columns_shared_with_the_location() {
        assert!(UPDATE_AGENT_RESULT_SEQUENCE_SQL.contains("GREATEST(agent.updated_at, $4)"));
        assert!(!UPDATE_AGENT_RESULT_SEQUENCE_SQL.contains("GREATEST(updated_at, $4)"));
    }

    #[test]
    fn result_page_filters_remain_organization_scoped() {
        assert!(RESULT_PAGE_FILTERS.starts_with("organization_id = $1"));
        assert!(
            RESULT_PAGE_FILTERS
                .contains("monitor.organization_id = synthetic_results.organization_id")
        );
        assert!(RESULT_PAGE_FILTERS.contains("monitor.id = synthetic_results.monitor_id"));
        assert!(RESULT_PAGE_FILTERS.contains("outcome = $3"));
        assert!(RESULT_PAGE_FILTERS.contains("location_id = $4"));
    }
}
