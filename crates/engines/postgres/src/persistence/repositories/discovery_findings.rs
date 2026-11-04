// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `discovery_findings` 表：自动发现找到的问题。

use async_trait::async_trait;
use serde_json::Value;
use sqlx::{PgPool, Postgres, QueryBuilder, Row};

use super::sqlx_err;
use crate::{
    domain::alerting::{
        discovery::{DiscoveryRepository, Finding, FindingFilter, FindingKind},
        incident::Severity,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgDiscoveryRepository {
    pool: PgPool,
}

impl PgDiscoveryRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, kind, finding_key, severity, subject, environment, metric, title,
     value, baseline, started_at, last_seen_at, ended_at, missed_rounds,
     detail, created_at, updated_at";

fn row_to(row: sqlx::postgres::PgRow) -> Result<Finding> {
    let kind: String = row.try_get("kind").map_err(sqlx_err)?;
    let severity: String = row.try_get("severity").map_err(sqlx_err)?;

    Ok(Finding {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        kind: FindingKind::parse(&kind)
            .ok_or_else(|| Error::internal(format!("unknown discovery finding kind {kind}")))?,
        key: row.try_get("finding_key").map_err(sqlx_err)?,
        severity: Severity::from_label(&severity).unwrap_or(Severity::Warning),
        subject: row.try_get("subject").map_err(sqlx_err)?,
        environment: row.try_get("environment").map_err(sqlx_err)?,
        metric: row.try_get("metric").map_err(sqlx_err)?,
        title: row.try_get("title").map_err(sqlx_err)?,
        value: row.try_get("value").map_err(sqlx_err)?,
        baseline: row.try_get("baseline").map_err(sqlx_err)?,
        started_at: row
            .try_get::<TimestampMicros, _>("started_at")
            .map_err(sqlx_err)?,
        last_seen_at: row
            .try_get::<TimestampMicros, _>("last_seen_at")
            .map_err(sqlx_err)?,
        ended_at: row
            .try_get::<Option<TimestampMicros>, _>("ended_at")
            .map_err(sqlx_err)?,
        missed_rounds: u32::try_from(row.try_get::<i32, _>("missed_rounds").map_err(sqlx_err)?)
            .unwrap_or_default(),
        detail: row.try_get::<Value, _>("detail").map_err(sqlx_err)?,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(sqlx_err)?,
    })
}

#[async_trait]
impl DiscoveryRepository for PgDiscoveryRepository {
    async fn list_open(&self, org_id: &Id) -> Result<Vec<Finding>> {
        sqlx::query(&format!(
            "SELECT {COLS} FROM discovery_findings
             WHERE org_id = $1 AND ended_at IS NULL"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?
        .into_iter()
        .map(row_to)
        .collect()
    }

    async fn list(&self, org_id: &Id, filter: &FindingFilter) -> Result<Vec<Finding>> {
        let mut query: QueryBuilder<Postgres> = QueryBuilder::new(format!(
            "SELECT {COLS} FROM discovery_findings WHERE org_id = "
        ));
        query.push_bind(&org_id.0);

        match filter.open {
            Some(true) => {
                query.push(" AND ended_at IS NULL");
            }
            Some(false) => {
                query.push(" AND ended_at IS NOT NULL");
            }
            None => {}
        }

        if let Some(kind) = filter.kind {
            query.push(" AND kind = ").push_bind(kind.as_str());
        }

        if let Some(since) = filter.since {
            query.push(" AND last_seen_at >= ").push_bind(since);
        }

        query
            .push(" ORDER BY last_seen_at DESC, id LIMIT ")
            .push_bind(i64::try_from(filter.limit.max(1)).unwrap_or(i64::MAX));

        query
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?
            .into_iter()
            .map(row_to)
            .collect()
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<Option<Finding>> {
        sqlx::query(&format!(
            "SELECT {COLS} FROM discovery_findings WHERE org_id = $1 AND id = $2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .map(row_to)
        .transpose()
    }

    async fn save(&self, findings: &[Finding]) -> Result<()> {
        if findings.is_empty() {
            return Ok(());
        }

        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        for finding in findings {
            sqlx::query(
                "INSERT INTO discovery_findings
                     (id, org_id, kind, finding_key, severity, subject, environment, metric, title,
                      value, baseline, started_at, last_seen_at, ended_at,
                      missed_rounds, detail, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16,
                         $17, $18)
                 ON CONFLICT (id) DO UPDATE SET
                     severity = EXCLUDED.severity,
                     title = EXCLUDED.title,
                     value = EXCLUDED.value,
                     baseline = EXCLUDED.baseline,
                     last_seen_at = EXCLUDED.last_seen_at,
                     ended_at = EXCLUDED.ended_at,
                     missed_rounds = EXCLUDED.missed_rounds,
                     detail = EXCLUDED.detail,
                     updated_at = EXCLUDED.updated_at",
            )
            .bind(&finding.id.0)
            .bind(&finding.org_id.0)
            .bind(finding.kind.as_str())
            .bind(&finding.key)
            .bind(finding.severity.as_str())
            .bind(&finding.subject)
            .bind(&finding.environment)
            .bind(&finding.metric)
            .bind(&finding.title)
            .bind(finding.value)
            .bind(finding.baseline)
            .bind(finding.started_at)
            .bind(finding.last_seen_at)
            .bind(finding.ended_at)
            .bind(i32::try_from(finding.missed_rounds).unwrap_or(i32::MAX))
            .bind(&finding.detail)
            .bind(finding.created_at)
            .bind(finding.updated_at)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }

        tx.commit().await.map_err(sqlx_err)
    }

    async fn delete_ended_before(&self, before: TimestampMicros) -> Result<u64> {
        let done = sqlx::query(
            "DELETE FROM discovery_findings
             WHERE ended_at IS NOT NULL AND ended_at < $1",
        )
        .bind(before)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(done.rows_affected())
    }
}
