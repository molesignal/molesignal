// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The rules people set for grouping errors, read whole by the projector.

use sqlx::{PgPool, Row, postgres::PgRow};

use crate::{
    domain::apm::{ErrorRule, ErrorRuleAction},
    infra::persistence::sqlx_err,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const COLUMNS: &str = "id, org_id, name, service_namespace, service_name, error_type, message,
    frame, endpoint, action, enabled, position, updated_by, updated_at";

/// One organization's rules, or every organization's, in the order they are tried.
pub(super) async fn list(pool: &PgPool, org_id: Option<&Id>) -> Result<Vec<(Id, ErrorRule)>> {
    let rows = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM apm_error_rules
         WHERE $1::TEXT IS NULL OR org_id = $1
         ORDER BY org_id, position, id"
    ))
    .bind(org_id.map(Id::as_str))
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;

    rows.iter().map(from_row).collect()
}

pub(super) async fn save(pool: &PgPool, org_id: &Id, rule: &ErrorRule) -> Result<()> {
    let position = i32::try_from(rule.position).unwrap_or(i32::MAX);

    sqlx::query(
        "INSERT INTO apm_error_rules (
             id, org_id, name, service_namespace, service_name, error_type, message, frame,
             endpoint, action, enabled, position, updated_by, updated_at
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
         ON CONFLICT (id) DO UPDATE
         SET name = EXCLUDED.name,
             service_namespace = EXCLUDED.service_namespace,
             service_name = EXCLUDED.service_name,
             error_type = EXCLUDED.error_type,
             message = EXCLUDED.message,
             frame = EXCLUDED.frame,
             endpoint = EXCLUDED.endpoint,
             action = EXCLUDED.action,
             enabled = EXCLUDED.enabled,
             position = EXCLUDED.position,
             updated_by = EXCLUDED.updated_by,
             updated_at = EXCLUDED.updated_at
         WHERE apm_error_rules.org_id = EXCLUDED.org_id",
    )
    .bind(rule.id.as_str())
    .bind(org_id.as_str())
    .bind(&rule.name)
    .bind(&rule.service_namespace)
    .bind(&rule.service_name)
    .bind(&rule.error_type)
    .bind(&rule.message)
    .bind(&rule.frame)
    .bind(&rule.endpoint)
    .bind(rule.action.as_str())
    .bind(rule.enabled)
    .bind(position)
    .bind(rule.updated_by.as_str())
    .bind(rule.updated_at)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

pub(super) async fn delete(pool: &PgPool, org_id: &Id, id: &Id) -> Result<bool> {
    let result = sqlx::query("DELETE FROM apm_error_rules WHERE org_id = $1 AND id = $2")
        .bind(org_id.as_str())
        .bind(id.as_str())
        .execute(pool)
        .await
        .map_err(sqlx_err)?;

    Ok(result.rows_affected() > 0)
}

/// Numbers the named rules in order from 0 and the rest after them, as they were.
pub(super) async fn order(pool: &PgPool, org_id: &Id, ids: &[Id]) -> Result<()> {
    let ids = ids.iter().map(Id::as_str).collect::<Vec<_>>();

    sqlx::query(
        "UPDATE apm_error_rules AS rules
         SET position = ranked.position
         FROM (
             SELECT id, (ROW_NUMBER() OVER (
                 ORDER BY COALESCE(array_position($2::TEXT[], id), 2147483647), position, id
             ) - 1)::INTEGER AS position
             FROM apm_error_rules
             WHERE org_id = $1
         ) AS ranked
         WHERE rules.org_id = $1 AND rules.id = ranked.id",
    )
    .bind(org_id.as_str())
    .bind(&ids)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

fn from_row(row: &PgRow) -> Result<(Id, ErrorRule)> {
    let action: String = row.try_get("action").map_err(sqlx_err)?;
    let position: i32 = row.try_get("position").map_err(sqlx_err)?;
    let text = |column: &str| row.try_get::<String, _>(column).map_err(sqlx_err);

    Ok((
        Id::from_string(text("org_id")?),
        ErrorRule {
            id: Id::from_string(text("id")?),
            name: text("name")?,
            service_namespace: text("service_namespace")?,
            service_name: text("service_name")?,
            error_type: text("error_type")?,
            message: text("message")?,
            frame: text("frame")?,
            endpoint: text("endpoint")?,
            action: ErrorRuleAction::parse(&action).ok_or_else(|| {
                Error::internal(format!("unknown APM error rule action {action}"))
            })?,
            enabled: row.try_get("enabled").map_err(sqlx_err)?,
            position: u32::try_from(position).unwrap_or_default(),
            updated_by: Id::from_string(text("updated_by")?),
            updated_at: row
                .try_get::<TimestampMicros, _>("updated_at")
                .map_err(sqlx_err)?,
        },
    ))
}
