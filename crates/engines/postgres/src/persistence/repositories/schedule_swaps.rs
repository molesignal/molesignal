// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::{
    domain::alerting::swap::{SwapRequest, SwapRequestRepository, SwapStatus},
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgSwapRequestRepository {
    pool: PgPool,
}

impl PgSwapRequestRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, schedule_id, requester_id, target_id, start_at,
    end_at, note, status, override_id, decided_by, decided_at,
    created_at, updated_at";

/// 一个排班或一个人最多列多少条申请。
const MOST_SWAPS: i64 = 200;

fn row_to(row: sqlx::postgres::PgRow) -> Result<SwapRequest> {
    let optional_id = |column: &str| -> Result<Option<Id>> {
        Ok(row
            .try_get::<Option<String>, _>(column)
            .map_err(sqlx_err)?
            .map(Id))
    };

    Ok(SwapRequest {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        schedule_id: Id(row.try_get("schedule_id").map_err(sqlx_err)?),
        requester_id: Id(row.try_get("requester_id").map_err(sqlx_err)?),
        target_id: Id(row.try_get("target_id").map_err(sqlx_err)?),
        start_at: row
            .try_get::<TimestampMicros, _>("start_at")
            .map_err(sqlx_err)?,
        end_at: row
            .try_get::<TimestampMicros, _>("end_at")
            .map_err(sqlx_err)?,
        note: row.try_get("note").map_err(sqlx_err)?,
        status: SwapStatus::parse(&row.try_get::<String, _>("status").map_err(sqlx_err)?)?,
        override_id: optional_id("override_id")?,
        decided_by: optional_id("decided_by")?,
        decided_at: row
            .try_get::<Option<TimestampMicros>, _>("decided_at")
            .map_err(sqlx_err)?,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(sqlx_err)?,
    })
}

#[async_trait]
impl SwapRequestRepository for PgSwapRequestRepository {
    async fn create(&self, request: SwapRequest) -> Result<SwapRequest> {
        sqlx::query(&format!(
            "INSERT INTO schedule_swap_requests ({COLS})
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)"
        ))
        .bind(&request.id.0)
        .bind(&request.org_id.0)
        .bind(&request.schedule_id.0)
        .bind(&request.requester_id.0)
        .bind(&request.target_id.0)
        .bind(request.start_at)
        .bind(request.end_at)
        .bind(&request.note)
        .bind(request.status.as_str())
        .bind(request.override_id.as_ref().map(|id| &id.0))
        .bind(request.decided_by.as_ref().map(|id| &id.0))
        .bind(request.decided_at)
        .bind(request.created_at)
        .bind(request.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(request)
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<Option<SwapRequest>> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM schedule_swap_requests WHERE org_id = $1 AND id = $2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row.map(row_to).transpose()
    }

    async fn update_if_status(&self, request: SwapRequest, expected: SwapStatus) -> Result<bool> {
        let updated = sqlx::query(
            "UPDATE schedule_swap_requests
                SET status = $3, override_id = $4, decided_by = $5, decided_at = $6,
                    updated_at = $7
              WHERE org_id = $1 AND id = $2 AND status = $8",
        )
        .bind(&request.org_id.0)
        .bind(&request.id.0)
        .bind(request.status.as_str())
        .bind(request.override_id.as_ref().map(|id| &id.0))
        .bind(request.decided_by.as_ref().map(|id| &id.0))
        .bind(request.decided_at)
        .bind(request.updated_at)
        .bind(expected.as_str())
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(updated.rows_affected() > 0)
    }

    async fn list_for_schedule(&self, org_id: &Id, schedule_id: &Id) -> Result<Vec<SwapRequest>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM schedule_swap_requests
              WHERE org_id = $1 AND schedule_id = $2
           ORDER BY created_at DESC
              LIMIT $3"
        ))
        .bind(&org_id.0)
        .bind(&schedule_id.0)
        .bind(MOST_SWAPS)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to).collect()
    }

    async fn list_for_user(&self, org_id: &Id, user_id: &Id) -> Result<Vec<SwapRequest>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM schedule_swap_requests
              WHERE org_id = $1 AND (requester_id = $2 OR target_id = $2)
           ORDER BY created_at DESC
              LIMIT $3"
        ))
        .bind(&org_id.0)
        .bind(&user_id.0)
        .bind(MOST_SWAPS)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to).collect()
    }
}
