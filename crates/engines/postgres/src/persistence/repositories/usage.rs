// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::shared::{Result, ids::Id};

pub const HOUR_MICROS: i64 = 3_600 * 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntakeUsageBucket {
    pub bucket_start_micros: i64,
    pub intake_bytes: i64,
}

pub fn hour_bucket_start(timestamp_micros: i64) -> i64 {
    timestamp_micros.div_euclid(HOUR_MICROS) * HOUR_MICROS
}

#[async_trait]
pub trait UsageRepository: Send + Sync {
    /// 累加某 org 在给定小时内收到的原始 payload 字节。
    async fn add_hourly_intake_bytes(
        &self,
        org_id: &Id,
        timestamp_micros: i64,
        bytes: i64,
    ) -> Result<()>;
    /// 读取与 `[start_micros, end_micros]` 相交的小时桶，按时间升序返回。
    async fn hourly_intake_bytes(
        &self,
        org_id: &Id,
        start_micros: i64,
        end_micros: i64,
    ) -> Result<Vec<IntakeUsageBucket>>;
}

pub struct PgUsageRepository {
    pool: PgPool,
}

impl PgUsageRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UsageRepository for PgUsageRepository {
    async fn add_hourly_intake_bytes(
        &self,
        org_id: &Id,
        timestamp_micros: i64,
        bytes: i64,
    ) -> Result<()> {
        let bucket_start_micros = hour_bucket_start(timestamp_micros);
        sqlx::query(
            "INSERT INTO intake_usage_hourly (org_id, bucket_start_micros, intake_bytes)
             VALUES ($1, $2, $3)
             ON CONFLICT (org_id, bucket_start_micros) DO UPDATE
                SET intake_bytes = intake_usage_hourly.intake_bytes + EXCLUDED.intake_bytes",
        )
        .bind(&org_id.0)
        .bind(bucket_start_micros)
        .bind(bytes)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn hourly_intake_bytes(
        &self,
        org_id: &Id,
        start_micros: i64,
        end_micros: i64,
    ) -> Result<Vec<IntakeUsageBucket>> {
        let first_bucket = hour_bucket_start(start_micros);
        let rows = sqlx::query(
            "SELECT bucket_start_micros, intake_bytes
             FROM intake_usage_hourly
             WHERE org_id = $1
               AND bucket_start_micros >= $2
               AND bucket_start_micros <= $3
             ORDER BY bucket_start_micros ASC",
        )
        .bind(&org_id.0)
        .bind(first_bucket)
        .bind(end_micros)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter()
            .map(|row| {
                Ok(IntakeUsageBucket {
                    bucket_start_micros: row.try_get("bucket_start_micros").map_err(sqlx_err)?,
                    intake_bytes: row.try_get("intake_bytes").map_err(sqlx_err)?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hour_bucket_start_uses_utc_epoch_boundaries() {
        assert_eq!(hour_bucket_start(0), 0);
        assert_eq!(hour_bucket_start(HOUR_MICROS - 1), 0);
        assert_eq!(hour_bucket_start(HOUR_MICROS), HOUR_MICROS);
        assert_eq!(hour_bucket_start(HOUR_MICROS + 42), HOUR_MICROS);
    }
}
