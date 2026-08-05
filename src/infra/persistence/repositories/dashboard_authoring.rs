// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::{
    dashboards::{COLS as DASHBOARD_COLS, row_to_dashboard},
    sqlx_err,
};
use crate::{
    domain::dashboard::{
        authoring::{
            ConsumeDashboardDraft, DashboardDraft, DashboardDraftRepository, DashboardDraftStatus,
            DraftConsumption, PreflightReport, PreflightWarningRecord,
        },
        contract_registry::DASHBOARD_AUTHORING_CAPABILITY,
    },
    shared::{Error, Result, contracts::ContractIssue, ids::Id, time::TimestampMicros},
};

pub struct PgDashboardDraftRepository {
    pool: PgPool,
}

impl PgDashboardDraftRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const DRAFT_COLS: &str = "id, org_id, created_by, authoring_version,
    model_schema_version, compiler_version, contract_binding_revision,
    authoring_schema_hash, model_schema_hash, visualization_schema_hash,
    authoring_spec, compiled_model, model_hash, folder_id, status, dashboard_id, warnings, preflight,
    created_at_micros, expires_at_micros, consumed_at_micros";

fn row_to_draft(row: sqlx::postgres::PgRow) -> Result<DashboardDraft> {
    let status: String = row.try_get("status").map_err(sqlx_err)?;
    let status = DashboardDraftStatus::parse(&status)
        .ok_or_else(|| Error::internal("unknown Dashboard draft status"))?;
    let authoring_version: i32 = row.try_get("authoring_version").map_err(sqlx_err)?;
    let model_schema_version: i32 = row.try_get("model_schema_version").map_err(sqlx_err)?;
    let authoring_spec: Json<serde_json::Value> =
        row.try_get("authoring_spec").map_err(sqlx_err)?;
    let compiled_model: Json<serde_json::Value> =
        row.try_get("compiled_model").map_err(sqlx_err)?;
    let warnings: Json<Vec<PreflightWarningRecord>> = row.try_get("warnings").map_err(sqlx_err)?;
    let preflight: Json<PreflightReport> = row.try_get("preflight").map_err(sqlx_err)?;
    Ok(DashboardDraft {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        created_by: Id::from_string(row.try_get::<String, _>("created_by").map_err(sqlx_err)?),
        authoring_version: authoring_version as u32,
        model_schema_version: model_schema_version as u32,
        compiler_version: row.try_get("compiler_version").map_err(sqlx_err)?,
        contract_binding_revision: row.try_get("contract_binding_revision").map_err(sqlx_err)?,
        authoring_schema_hash: row.try_get("authoring_schema_hash").map_err(sqlx_err)?,
        model_schema_hash: row.try_get("model_schema_hash").map_err(sqlx_err)?,
        visualization_schema_hash: row.try_get("visualization_schema_hash").map_err(sqlx_err)?,
        authoring_spec: authoring_spec.0,
        compiled_model: compiled_model.0,
        model_hash: row.try_get("model_hash").map_err(sqlx_err)?,
        folder_id: row
            .try_get::<Option<String>, _>("folder_id")
            .map_err(sqlx_err)?
            .map(Id::from_string),
        status,
        dashboard_id: row
            .try_get::<Option<String>, _>("dashboard_id")
            .map_err(sqlx_err)?
            .map(Id::from_string),
        warnings: warnings.0,
        preflight: preflight.0,
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        expires_at: TimestampMicros(row.try_get("expires_at_micros").map_err(sqlx_err)?),
        consumed_at: row
            .try_get::<Option<i64>, _>("consumed_at_micros")
            .map_err(sqlx_err)?
            .map(TimestampMicros),
    })
}

#[async_trait]
impl DashboardDraftRepository for PgDashboardDraftRepository {
    async fn create(&self, draft: DashboardDraft) -> Result<DashboardDraft> {
        sqlx::query(
            "INSERT INTO intelligence_dashboard_drafts
             (id, org_id, created_by, authoring_version, model_schema_version,
              compiler_version, contract_binding_revision, authoring_schema_hash,
              model_schema_hash, visualization_schema_hash, authoring_spec, compiled_model,
              model_hash, folder_id, status, dashboard_id, warnings, preflight,
              created_at_micros, expires_at_micros, consumed_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
                     $13, $14, $15, $16, $17, $18, $19, $20, $21)",
        )
        .bind(&draft.id.0)
        .bind(&draft.org_id.0)
        .bind(&draft.created_by.0)
        .bind(draft.authoring_version as i32)
        .bind(draft.model_schema_version as i32)
        .bind(&draft.compiler_version)
        .bind(draft.contract_binding_revision)
        .bind(&draft.authoring_schema_hash)
        .bind(&draft.model_schema_hash)
        .bind(&draft.visualization_schema_hash)
        .bind(Json(&draft.authoring_spec))
        .bind(Json(&draft.compiled_model))
        .bind(&draft.model_hash)
        .bind(draft.folder_id.as_ref().map(|value| &value.0))
        .bind(draft.status.as_str())
        .bind(draft.dashboard_id.as_ref().map(|value| &value.0))
        .bind(Json(&draft.warnings))
        .bind(Json(&draft.preflight))
        .bind(draft.created_at.0)
        .bind(draft.expires_at.0)
        .bind(draft.consumed_at.map(|value| value.0))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(draft)
    }

    async fn get(
        &self,
        org_id: &Id,
        draft_id: &Id,
        now: TimestampMicros,
    ) -> Result<DashboardDraft> {
        sqlx::query(
            "UPDATE intelligence_dashboard_drafts
             SET status = 'expired'
             WHERE id = $1 AND org_id = $2 AND status = 'ready'
               AND expires_at_micros <= $3",
        )
        .bind(&draft_id.0)
        .bind(&org_id.0)
        .bind(now.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        let row = sqlx::query(&format!(
            "SELECT {DRAFT_COLS} FROM intelligence_dashboard_drafts
             WHERE id = $1 AND org_id = $2"
        ))
        .bind(&draft_id.0)
        .bind(&org_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(draft_not_found)?;
        row_to_draft(row)
    }

    async fn consume_and_create(&self, request: ConsumeDashboardDraft) -> Result<DraftConsumption> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let row = sqlx::query(&format!(
            "SELECT {DRAFT_COLS} FROM intelligence_dashboard_drafts
             WHERE id = $1 AND org_id = $2 FOR UPDATE"
        ))
        .bind(&request.draft_id.0)
        .bind(&request.org_id.0)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(draft_not_found)?;
        let draft = row_to_draft(row)?;
        if draft.created_by != request.actor {
            return Err(draft_not_found());
        }
        if draft.status == DashboardDraftStatus::Consumed {
            let dashboard_id = draft
                .dashboard_id
                .ok_or_else(|| Error::internal("consumed draft is missing dashboard_id"))?;
            let dashboard = sqlx::query(&format!(
                "SELECT {DASHBOARD_COLS} FROM dashboards WHERE id = $1 AND org_id = $2"
            ))
            .bind(&dashboard_id.0)
            .bind(&request.org_id.0)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| Error::internal("consumed draft Dashboard is missing"))?;
            transaction.commit().await.map_err(sqlx_err)?;
            return Ok(DraftConsumption::Replay(row_to_dashboard(dashboard)?));
        }
        if draft.status == DashboardDraftStatus::Expired || draft.expires_at <= request.now {
            sqlx::query(
                "UPDATE intelligence_dashboard_drafts SET status = 'expired'
                 WHERE id = $1 AND org_id = $2 AND status = 'ready'",
            )
            .bind(&request.draft_id.0)
            .bind(&request.org_id.0)
            .execute(&mut *transaction)
            .await
            .map_err(sqlx_err)?;
            transaction.commit().await.map_err(sqlx_err)?;
            return Err(draft_issue(
                "DRAFT_EXPIRED",
                "Dashboard draft has expired; prepare it again",
            ));
        }
        if draft.model_hash != request.expected_hash {
            return Err(draft_issue(
                "DRAFT_HASH_MISMATCH",
                "Dashboard draft hash does not match the reviewed preview",
            ));
        }
        if draft.compiler_version != request.compiler_version {
            return Err(draft_issue(
                "DRAFT_STALE",
                "Dashboard draft was compiled by an incompatible compiler revision",
            ));
        }
        ensure_active_contract_binding(&mut transaction, &draft).await?;
        validate_candidate(&request)?;
        insert_dashboard(&mut transaction, &request.dashboard).await?;
        sqlx::query(
            "UPDATE intelligence_dashboard_drafts
             SET status = 'consumed', dashboard_id = $3, consumed_at_micros = $4
             WHERE id = $1 AND org_id = $2 AND status = 'ready'",
        )
        .bind(&request.draft_id.0)
        .bind(&request.org_id.0)
        .bind(&request.dashboard.id.0)
        .bind(request.now.0)
        .execute(&mut *transaction)
        .await
        .map_err(sqlx_err)?;
        transaction.commit().await.map_err(sqlx_err)?;
        Ok(DraftConsumption::Created(request.dashboard))
    }
}

async fn ensure_active_contract_binding(
    transaction: &mut sqlx::PgConnection,
    draft: &DashboardDraft,
) -> Result<()> {
    let row = sqlx::query(
        "SELECT revision, authoring_schema_hash, model_schema_hash,
                visualization_schema_hash, compiler_version, enabled
         FROM intelligence_capability_contract_bindings
         WHERE capability_key = $1
         FOR SHARE",
    )
    .bind(DASHBOARD_AUTHORING_CAPABILITY)
    .fetch_optional(transaction)
    .await
    .map_err(sqlx_err)?
    .ok_or_else(|| {
        draft_issue(
            "DRAFT_STALE",
            "Dashboard authoring contract binding is unavailable",
        )
    })?;
    let matches = row.try_get::<bool, _>("enabled").map_err(sqlx_err)?
        && row.try_get::<i64, _>("revision").map_err(sqlx_err)? == draft.contract_binding_revision
        && row
            .try_get::<String, _>("authoring_schema_hash")
            .map_err(sqlx_err)?
            == draft.authoring_schema_hash
        && row
            .try_get::<String, _>("model_schema_hash")
            .map_err(sqlx_err)?
            == draft.model_schema_hash
        && row
            .try_get::<String, _>("visualization_schema_hash")
            .map_err(sqlx_err)?
            == draft.visualization_schema_hash
        && row
            .try_get::<String, _>("compiler_version")
            .map_err(sqlx_err)?
            == draft.compiler_version;
    if matches {
        Ok(())
    } else {
        Err(draft_issue(
            "DRAFT_STALE",
            "Dashboard draft contract revision is no longer active",
        ))
    }
}

async fn insert_dashboard(
    transaction: &mut sqlx::PgConnection,
    dashboard: &crate::domain::dashboard::Dashboard,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO dashboards
         (id, org_id, folder_id, uid, title, tags, model, version,
          created_at_micros, updated_at_micros, created_by, updated_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
    )
    .bind(&dashboard.id.0)
    .bind(&dashboard.org_id.0)
    .bind(dashboard.folder_id.as_ref().map(|value| &value.0))
    .bind(&dashboard.uid)
    .bind(&dashboard.title)
    .bind(Json(&dashboard.tags))
    .bind(Json(&dashboard.model))
    .bind(dashboard.version as i32)
    .bind(dashboard.created_at.0)
    .bind(dashboard.updated_at.0)
    .bind(&dashboard.created_by.0)
    .bind(&dashboard.updated_by.0)
    .execute(&mut *transaction)
    .await
    .map_err(sqlx_err)?;
    Ok(())
}

fn validate_candidate(request: &ConsumeDashboardDraft) -> Result<()> {
    let dashboard = &request.dashboard;
    if dashboard.org_id != request.org_id || dashboard.created_by != request.actor {
        return Err(Error::internal(
            "atomic Dashboard draft consumption received mismatched trusted identity",
        ));
    }
    Ok(())
}

fn draft_not_found() -> Error {
    Error::not_found("dashboard draft not found")
}

fn draft_issue(code: &str, message: &str) -> Error {
    Error::validation(
        "Dashboard draft cannot be consumed",
        vec![ContractIssue::new(code, "/draft_id", message, true)],
    )
}
