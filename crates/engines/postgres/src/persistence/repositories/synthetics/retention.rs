// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Old runs, expired artifacts and finished tasks, deleted in batches. The objects an artifact or
//! a run's detail was stored under are returned for the caller to delete once the rows are gone:
//! an object left over wastes space, a row left pointing at nothing would mislead.

use sqlx::Row;

use super::PgSyntheticRepository;
use crate::{
    domain::synthetics::PurgedResults,
    shared::{Result, time::TimestampMicros},
};

pub(super) async fn purge_results(
    repository: &PgSyntheticRepository,
    before: TimestampMicros,
    limit: u32,
) -> Result<PurgedResults> {
    let mut transaction = sqlx::begin(&repository.pool)
        .await
        .map_err(super::sqlx_err)?;

    // A run a location shows as its latest, or a revision as the test that let it publish, stays
    // with them; the next run or test takes its place.
    let doomed: Vec<(String, Option<String>)> = sqlx::query(
        "SELECT result.id, result.detail_object_key
         FROM synthetic_results result
         WHERE result.received_at < $1
           AND NOT EXISTS (
               SELECT 1 FROM synthetic_monitor_location_states state
               WHERE state.last_result_id = result.id
           )
           AND NOT EXISTS (
               SELECT 1 FROM synthetic_monitor_revisions revision
               WHERE revision.last_test_result_id = result.id
           )
         ORDER BY result.received_at
         LIMIT $2
         FOR UPDATE OF result SKIP LOCKED",
    )
    .bind(before)
    .bind(i64::from(limit.max(1)))
    .fetch_all(&mut *transaction)
    .await
    .map_err(super::sqlx_err)?
    .into_iter()
    .map(|row| {
        Ok((
            row.try_get("id").map_err(super::sqlx_err)?,
            row.try_get("detail_object_key").map_err(super::sqlx_err)?,
        ))
    })
    .collect::<Result<_>>()?;

    if doomed.is_empty() {
        return Ok(PurgedResults::default());
    }

    let ids = doomed.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();
    let mut object_keys = doomed
        .into_iter()
        .filter_map(|(_, key)| key)
        .collect::<Vec<_>>();

    let artifacts: Vec<String> = sqlx::query_scalar(
        "SELECT object_key FROM synthetic_result_artifacts WHERE result_id = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(&mut *transaction)
    .await
    .map_err(super::sqlx_err)?;

    object_keys.extend(artifacts);

    // A state change keeps its place in the history, without the run that set it off.
    sqlx::query(
        "UPDATE synthetic_state_transitions SET result_id = NULL WHERE result_id = ANY($1)",
    )
    .bind(&ids)
    .execute(&mut *transaction)
    .await
    .map_err(super::sqlx_err)?;

    // The artifacts go with their runs.
    let deleted = sqlx::query("DELETE FROM synthetic_results WHERE id = ANY($1)")
        .bind(&ids)
        .execute(&mut *transaction)
        .await
        .map_err(super::sqlx_err)?
        .rows_affected();

    transaction.commit().await.map_err(super::sqlx_err)?;

    Ok(PurgedResults {
        deleted,
        object_keys,
    })
}

pub(super) async fn purge_expired_artifacts(
    repository: &PgSyntheticRepository,
    now: TimestampMicros,
    limit: u32,
) -> Result<Vec<String>> {
    sqlx::query_scalar(
        "DELETE FROM synthetic_result_artifacts
         WHERE id IN (
             SELECT id FROM synthetic_result_artifacts
             WHERE expires_at < $1
             ORDER BY expires_at
             LIMIT $2
         )
         RETURNING object_key",
    )
    .bind(now)
    .bind(i64::from(limit.max(1)))
    .fetch_all(&repository.pool)
    .await
    .map_err(super::sqlx_err)
}

pub(super) async fn purge_finished_tasks(
    repository: &PgSyntheticRepository,
    before: TimestampMicros,
    limit: u32,
) -> Result<u64> {
    let deleted = sqlx::query(
        "DELETE FROM synthetic_probe_tasks
         WHERE id IN (
             SELECT task.id FROM synthetic_probe_tasks task
             WHERE task.state IN ('completed', 'expired', 'cancelled', 'skipped')
               AND task.updated_at < $1
               AND NOT EXISTS (
                   SELECT 1 FROM synthetic_results result WHERE result.task_id = task.id
               )
             ORDER BY task.updated_at
             LIMIT $2
         )",
    )
    .bind(before)
    .bind(i64::from(limit.max(1)))
    .execute(&repository.pool)
    .await
    .map_err(super::sqlx_err)?
    .rows_affected();

    Ok(deleted)
}
