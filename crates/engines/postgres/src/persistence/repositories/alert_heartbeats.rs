// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `alert_heartbeats` 表：心跳规则的打卡地址与最后一次打卡。

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::{
    domain::alerting::heartbeat::{Heartbeat, HeartbeatRepository, HeartbeatStatus},
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgHeartbeatRepository {
    pool: PgPool,
}

impl PgHeartbeatRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "rule_id, org_id, token_hash, last_checkin_at, last_status, last_message,
     created_at";

fn row_to(row: sqlx::postgres::PgRow) -> Result<Heartbeat> {
    Ok(Heartbeat {
        rule_id: Id(row.try_get("rule_id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        token_hash: row.try_get("token_hash").map_err(sqlx_err)?,
        last_checkin_at: row
            .try_get::<Option<TimestampMicros>, _>("last_checkin_at")
            .map_err(sqlx_err)?,
        last_status: HeartbeatStatus::parse(
            &row.try_get::<String, _>("last_status").map_err(sqlx_err)?,
        ),
        last_message: row.try_get("last_message").map_err(sqlx_err)?,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
    })
}

#[async_trait]
impl HeartbeatRepository for PgHeartbeatRepository {
    async fn replace_token(
        &self,
        org_id: &Id,
        rule_id: &Id,
        token_hash: &str,
        now: TimestampMicros,
    ) -> Result<Heartbeat> {
        let row = sqlx::query(&format!(
            "INSERT INTO alert_heartbeats (rule_id, org_id, token_hash, created_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (rule_id) DO UPDATE
             SET token_hash = EXCLUDED.token_hash, created_at = EXCLUDED.created_at
             RETURNING {COLS}"
        ))
        .bind(&rule_id.0)
        .bind(&org_id.0)
        .bind(token_hash)
        .bind(now)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row_to(row)
    }

    async fn get(&self, rule_id: &Id) -> Result<Option<Heartbeat>> {
        sqlx::query(&format!(
            "SELECT {COLS} FROM alert_heartbeats WHERE rule_id = $1"
        ))
        .bind(&rule_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .map(row_to)
        .transpose()
    }

    async fn find_by_token_hash(&self, token_hash: &str) -> Result<Option<Heartbeat>> {
        sqlx::query(&format!(
            "SELECT {COLS} FROM alert_heartbeats WHERE token_hash = $1"
        ))
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .map(row_to)
        .transpose()
    }

    async fn record_checkin(
        &self,
        rule_id: &Id,
        at: TimestampMicros,
        status: HeartbeatStatus,
        message: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE alert_heartbeats
                SET last_checkin_at = GREATEST(COALESCE(last_checkin_at, $2), $2),
                    last_status = $3,
                    last_message = $4
              WHERE rule_id = $1",
        )
        .bind(&rule_id.0)
        .bind(at)
        .bind(status.as_str())
        .bind(message)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }
}
