// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Pre-approvals of remediation actions (a JSONB column of the action) and the organization's
//! switch (an `org_settings` section). A pre-approved run is claimed under a transaction-scoped
//! advisory lock on its action, so two requests at the same time never both take the last run the
//! pre-approval allows.

use async_trait::async_trait;
use domain::alerting::remediation::{
    RemediationPreapproval, RemediationRun,
    preapproval::{PreapprovalRepository, PreapprovalSettings},
};
use kernel::{Result, ids::Id, time::TimestampMicros};
use sqlx::types::Json;

use super::{
    super::{org_settings, sqlx_err},
    PgRemediationRepository,
    runs::insert_run,
};

/// How many runs of an action went out under its pre-approval since `$3`.
const PREAPPROVED_RUNS: &str = "SELECT COUNT(*) FROM remediation_runs
     WHERE org_id = $1 AND action_id = $2 AND preapproved AND created_at >= $3";

#[async_trait]
impl PreapprovalRepository for PgRemediationRepository {
    async fn save_preapproval(
        &self,
        org_id: &Id,
        action_id: &Id,
        preapproval: Option<&RemediationPreapproval>,
    ) -> Result<bool> {
        let updated = sqlx::query(
            "UPDATE remediation_actions SET preapproval = $3 WHERE org_id = $1 AND id = $2",
        )
        .bind(&org_id.0)
        .bind(&action_id.0)
        .bind(preapproval.map(Json))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();

        Ok(updated > 0)
    }

    async fn preapproved_runs_since(
        &self,
        org_id: &Id,
        action_id: &Id,
        since: TimestampMicros,
    ) -> Result<u32> {
        let used: i64 = sqlx::query_scalar(PREAPPROVED_RUNS)
            .bind(&org_id.0)
            .bind(&action_id.0)
            .bind(since)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(u32::try_from(used).unwrap_or(u32::MAX))
    }

    async fn claim_preapproved_run(
        &self,
        mut run: RemediationRun,
        max_runs: u32,
        since: TimestampMicros,
    ) -> Result<Option<RemediationRun>> {
        run.preapproved = true;

        let mut transaction = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!(
                "remediation_preapproval:{}:{}",
                run.org_id.0, run.action_id.0
            ))
            .execute(&mut *transaction)
            .await
            .map_err(sqlx_err)?;

        let used: i64 = sqlx::query_scalar(PREAPPROVED_RUNS)
            .bind(&run.org_id.0)
            .bind(&run.action_id.0)
            .bind(since)
            .fetch_one(&mut *transaction)
            .await
            .map_err(sqlx_err)?;

        if used >= i64::from(max_runs) {
            transaction.rollback().await.map_err(sqlx_err)?;
            return Ok(None);
        }

        insert_run(&mut transaction, &run).await?;
        transaction.commit().await.map_err(sqlx_err)?;

        Ok(Some(run))
    }

    async fn preapproval_settings(&self, org_id: &Id) -> Result<PreapprovalSettings> {
        Ok(org_settings::read::<PreapprovalSettings>(
            &self.pool,
            &org_id.0,
            org_settings::sections::REMEDIATION_PREAPPROVAL,
        )
        .await?
        .map(|section| section.value)
        .unwrap_or_default())
    }

    async fn save_preapproval_settings(
        &self,
        org_id: &Id,
        settings: PreapprovalSettings,
        updated_by: &Id,
        now: TimestampMicros,
    ) -> Result<()> {
        org_settings::write(
            &self.pool,
            &org_id.0,
            org_settings::sections::REMEDIATION_PREAPPROVAL,
            &settings,
            Some(&updated_by.0),
            now,
        )
        .await
    }
}
