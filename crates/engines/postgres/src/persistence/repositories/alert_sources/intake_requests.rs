// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `alert_intake_requests`：告警来源收到的请求。每次写入时把这个来源超出保留范围的旧记录删掉。

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::alerting::source::{
        IntakeAlert, IntakeRequest, IntakeRequestRepository, IntakeStatus, KEPT_REQUESTS,
        KEPT_REQUESTS_MICROS,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgIntakeRequestRepository {
    pool: PgPool,
}

impl PgIntakeRequestRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, source_id, received_at, via_cluster, status, http_status,
     message, alert_count, alerts, body_excerpt";

fn row_to(row: sqlx::postgres::PgRow) -> Result<IntakeRequest> {
    let status: String = row.try_get("status").map_err(sqlx_err)?;
    let alerts: Json<Vec<IntakeAlert>> = row.try_get("alerts").map_err(sqlx_err)?;

    Ok(IntakeRequest {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        source_id: Id::from_string(row.try_get::<String, _>("source_id").map_err(sqlx_err)?),
        received_at: row
            .try_get::<TimestampMicros, _>("received_at")
            .map_err(sqlx_err)?,
        via_cluster: row.try_get("via_cluster").map_err(sqlx_err)?,
        status: IntakeStatus::parse(&status)
            .ok_or_else(|| Error::internal(format!("unknown intake status `{status}`")))?,
        http_status: u16::try_from(row.try_get::<i32, _>("http_status").map_err(sqlx_err)?)
            .unwrap_or(0),
        message: row.try_get("message").map_err(sqlx_err)?,
        alert_count: u32::try_from(row.try_get::<i32, _>("alert_count").map_err(sqlx_err)?)
            .unwrap_or(0),
        alerts: alerts.0,
        body_excerpt: row.try_get("body_excerpt").map_err(sqlx_err)?,
    })
}

#[async_trait]
impl IntakeRequestRepository for PgIntakeRequestRepository {
    async fn record(&self, request: IntakeRequest) -> Result<()> {
        sqlx::query(
            "INSERT INTO alert_intake_requests
                (id, org_id, source_id, received_at, via_cluster, status, http_status,
                 message, alert_count, alerts, body_excerpt)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(&request.id.0)
        .bind(&request.org_id.0)
        .bind(&request.source_id.0)
        .bind(request.received_at)
        .bind(&request.via_cluster)
        .bind(request.status.as_str())
        .bind(i32::from(request.http_status))
        .bind(&request.message)
        .bind(i32::try_from(request.alert_count).unwrap_or(i32::MAX))
        .bind(Json(&request.alerts))
        .bind(&request.body_excerpt)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        // 只留最近 KEPT_REQUESTS 次、KEPT_REQUESTS_MICROS 以内的。
        sqlx::query(
            "DELETE FROM alert_intake_requests
              WHERE source_id = $1
                AND (received_at < $2
                     OR id IN (SELECT id FROM alert_intake_requests
                                WHERE source_id = $1
                                ORDER BY received_at DESC, id DESC
                               OFFSET $3))",
        )
        .bind(&request.source_id.0)
        .bind(TimestampMicros(
            request.received_at.0 - KEPT_REQUESTS_MICROS,
        ))
        .bind(i64::from(KEPT_REQUESTS))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn list(&self, org_id: &Id, source_id: &Id, limit: u32) -> Result<Vec<IntakeRequest>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM alert_intake_requests
              WHERE org_id = $1 AND source_id = $2
              ORDER BY received_at DESC, id DESC
              LIMIT $3"
        ))
        .bind(&org_id.0)
        .bind(&source_id.0)
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to).collect()
    }
}
