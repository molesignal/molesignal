// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The runs of remediation actions started on incidents: how a row reads and how one is written.

use std::collections::BTreeMap;

use domain::alerting::remediation::{RemediationRun, RemediationRunStatus};
use kernel::{Result, ids::Id, time::TimestampMicros};
use sqlx::{PgConnection, Row, types::Json};

use super::{super::sqlx_err, PgRemediationRepository};

pub(super) const RUN_COLS: &str =
    "id, org_id, incident_id, action_id, action_name, approval_id, inputs, status,
     output, verify_at, verification, requested_by, executed_by, preapproved, created_at,
     updated_at";

pub(super) fn row_to_run(row: &sqlx::postgres::PgRow) -> Result<RemediationRun> {
    let Json(inputs): Json<BTreeMap<String, String>> = row.try_get("inputs").map_err(sqlx_err)?;
    let status: String = row.try_get("status").map_err(sqlx_err)?;

    Ok(RemediationRun {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        incident_id: Id(row.try_get("incident_id").map_err(sqlx_err)?),
        action_id: Id(row.try_get("action_id").map_err(sqlx_err)?),
        action_name: row.try_get("action_name").map_err(sqlx_err)?,
        approval_id: Id(row.try_get("approval_id").map_err(sqlx_err)?),
        inputs,
        status: RemediationRunStatus::parse(&status),
        output: row.try_get("output").map_err(sqlx_err)?,
        verify_at: row
            .try_get::<Option<TimestampMicros>, _>("verify_at")
            .map_err(sqlx_err)?,
        verification: row.try_get("verification").map_err(sqlx_err)?,
        requested_by: Id(row.try_get("requested_by").map_err(sqlx_err)?),
        executed_by: row
            .try_get::<Option<String>, _>("executed_by")
            .map_err(sqlx_err)?
            .map(Id),
        preapproved: row.try_get("preapproved").map_err(sqlx_err)?,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(sqlx_err)?,
    })
}

/// Adds the row of a new run, on `connection` (a transaction when the caller holds one).
pub(super) async fn insert_run(connection: &mut PgConnection, run: &RemediationRun) -> Result<()> {
    sqlx::query(
        "INSERT INTO remediation_runs
            (id, org_id, incident_id, action_id, action_name, approval_id, inputs, status,
             output, verify_at, verification, requested_by, executed_by, preapproved,
             created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)",
    )
    .bind(&run.id.0)
    .bind(&run.org_id.0)
    .bind(&run.incident_id.0)
    .bind(&run.action_id.0)
    .bind(&run.action_name)
    .bind(&run.approval_id.0)
    .bind(Json(&run.inputs))
    .bind(run.status.as_str())
    .bind(&run.output)
    .bind(run.verify_at)
    .bind(&run.verification)
    .bind(&run.requested_by.0)
    .bind(run.executed_by.as_ref().map(|id| &id.0))
    .bind(run.preapproved)
    .bind(run.created_at)
    .bind(run.updated_at)
    .execute(connection)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

impl PgRemediationRepository {
    pub(super) async fn write_run(&self, run: &RemediationRun, insert: bool) -> Result<()> {
        if insert {
            let mut connection = self.pool.acquire().await.map_err(sqlx_err)?;
            return insert_run(&mut connection, run).await;
        }

        sqlx::query(
            "UPDATE remediation_runs
                SET status = $3, output = $4, verify_at = $5, verification = $6,
                    executed_by = $7, updated_at = $8
              WHERE id = $1 AND org_id = $2",
        )
        .bind(&run.id.0)
        .bind(&run.org_id.0)
        .bind(run.status.as_str())
        .bind(&run.output)
        .bind(run.verify_at)
        .bind(&run.verification)
        .bind(run.executed_by.as_ref().map(|id| &id.0))
        .bind(run.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }
}
