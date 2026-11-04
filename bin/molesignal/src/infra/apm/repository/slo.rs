// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Service level objectives as people set them. What an objective has met
//! is read from the APM buckets when asked, never stored.

use sqlx::{
    PgPool, Row,
    postgres::{PgRow, types::PgInterval},
};

use super::codec::{interval, interval_micros};
use crate::{
    domain::apm::{ServiceLevelObjective, SloKind, TransactionIdentity},
    infra::persistence::sqlx_err,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const COLUMNS: &str = "id, name, description, service_namespace, service_name, environment,
    transaction, kind, latency_threshold, objective, window_days, created_by,
    created_at, updated_by, updated_at";

pub(super) async fn list(pool: &PgPool, org_id: &Id) -> Result<Vec<ServiceLevelObjective>> {
    let rows = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM apm_slos WHERE org_id = $1
         ORDER BY service_namespace, service_name, environment, name"
    ))
    .bind(org_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;

    rows.iter().map(from_row).collect()
}

pub(super) async fn get(
    pool: &PgPool,
    org_id: &Id,
    id: &Id,
) -> Result<Option<ServiceLevelObjective>> {
    let row = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM apm_slos WHERE org_id = $1 AND id = $2"
    ))
    .bind(org_id.as_str())
    .bind(id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?;

    row.as_ref().map(from_row).transpose()
}

pub(super) async fn save(pool: &PgPool, org_id: &Id, slo: &ServiceLevelObjective) -> Result<()> {
    let transaction = slo
        .transaction
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| Error::internal(format!("encode SLO endpoint: {error}")))?;

    sqlx::query(
        "INSERT INTO apm_slos (
             id, org_id, name, description, service_namespace, service_name, environment,
             transaction, kind, latency_threshold, objective, window_days, created_by,
             created_at, updated_by, updated_at
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
         ON CONFLICT (id) DO UPDATE
         SET name = EXCLUDED.name,
             description = EXCLUDED.description,
             service_namespace = EXCLUDED.service_namespace,
             service_name = EXCLUDED.service_name,
             environment = EXCLUDED.environment,
             transaction = EXCLUDED.transaction,
             kind = EXCLUDED.kind,
             latency_threshold = EXCLUDED.latency_threshold,
             objective = EXCLUDED.objective,
             window_days = EXCLUDED.window_days,
             updated_by = EXCLUDED.updated_by,
             updated_at = EXCLUDED.updated_at
         WHERE apm_slos.org_id = EXCLUDED.org_id",
    )
    .bind(slo.id.as_str())
    .bind(org_id.as_str())
    .bind(&slo.name)
    .bind(&slo.description)
    .bind(&slo.namespace)
    .bind(&slo.service)
    .bind(&slo.environment)
    .bind(transaction)
    .bind(slo.kind.as_str())
    .bind(slo.latency_threshold.map(interval))
    .bind(slo.objective)
    .bind(slo.window_days as i16)
    .bind(slo.created_by.as_str())
    .bind(slo.created_at)
    .bind(slo.updated_by.as_str())
    .bind(slo.updated_at)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

pub(super) async fn delete(pool: &PgPool, org_id: &Id, id: &Id) -> Result<bool> {
    let result = sqlx::query("DELETE FROM apm_slos WHERE org_id = $1 AND id = $2")
        .bind(org_id.as_str())
        .bind(id.as_str())
        .execute(pool)
        .await
        .map_err(sqlx_err)?;

    Ok(result.rows_affected() > 0)
}

fn from_row(row: &PgRow) -> Result<ServiceLevelObjective> {
    let kind: String = row.try_get("kind").map_err(sqlx_err)?;
    let transaction: Option<serde_json::Value> = row.try_get("transaction").map_err(sqlx_err)?;
    let threshold: Option<PgInterval> = row.try_get("latency_threshold").map_err(sqlx_err)?;
    let window_days: i16 = row.try_get("window_days").map_err(sqlx_err)?;

    Ok(ServiceLevelObjective {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        name: row.try_get("name").map_err(sqlx_err)?,
        description: row.try_get("description").map_err(sqlx_err)?,
        namespace: row.try_get("service_namespace").map_err(sqlx_err)?,
        service: row.try_get("service_name").map_err(sqlx_err)?,
        environment: row.try_get("environment").map_err(sqlx_err)?,
        transaction: transaction
            .map(serde_json::from_value::<TransactionIdentity>)
            .transpose()
            .map_err(|error| Error::internal(format!("decode SLO endpoint: {error}")))?,
        kind: SloKind::parse(&kind)
            .ok_or_else(|| Error::internal(format!("unknown SLO kind {kind}")))?,
        latency_threshold: threshold.and_then(interval_micros),
        objective: row.try_get("objective").map_err(sqlx_err)?,
        window_days: window_days.max(0) as u16,
        created_by: Id::from_string(row.try_get::<String, _>("created_by").map_err(sqlx_err)?),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        updated_by: Id::from_string(row.try_get::<String, _>("updated_by").map_err(sqlx_err)?),
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(sqlx_err)?,
    })
}
