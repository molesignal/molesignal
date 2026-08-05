// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `license_usage_daily` + `ingest_usage_hourly` 表 Pg 实装（用量计量）。
//!
//! Per-org 每日 ingest 字节累计。ingest 计费门禁每批 upsert-increment 一次（按批不按
//! 事件，开销可控），供用量观测 / 出量上报基础。`day` 为 `YYYY-MM-DD`（UTC）。
//! 小时表记录所有部署的原始摄入字节，供首页等运营视图按时间窗读取；它不参与计费判定。

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::shared::{Result, ids::Id};

pub const HOUR_MICROS: i64 = 3_600 * 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IngestUsageBucket {
    pub bucket_start_micros: i64,
    pub ingest_bytes: i64,
}

pub fn hour_bucket_start(timestamp_micros: i64) -> i64 {
    timestamp_micros.div_euclid(HOUR_MICROS) * HOUR_MICROS
}

#[async_trait]
pub trait UsageRepository: Send + Sync {
    /// 累加某 org 当日 ingest 字节（upsert-increment）。
    async fn add_ingest_bytes(&self, org_id: &Id, day: &str, bytes: i64) -> Result<()>;
    /// 读取某 org 当日累计 ingest 字节；无记录返回 0。
    async fn get_ingest_bytes(&self, org_id: &Id, day: &str) -> Result<i64>;
    /// 累加某 org 在给定小时内收到的原始 payload 字节。
    async fn add_hourly_ingest_bytes(
        &self,
        org_id: &Id,
        timestamp_micros: i64,
        bytes: i64,
    ) -> Result<()>;
    /// 读取与 `[start_micros, end_micros]` 相交的小时桶，按时间升序返回。
    async fn hourly_ingest_bytes(
        &self,
        org_id: &Id,
        start_micros: i64,
        end_micros: i64,
    ) -> Result<Vec<IngestUsageBucket>>;
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
    async fn add_ingest_bytes(&self, org_id: &Id, day: &str, bytes: i64) -> Result<()> {
        sqlx::query(
            "INSERT INTO license_usage_daily (day, org_id, ingest_bytes, user_count)
             VALUES ($1, $2, $3, 0)
             ON CONFLICT (day, org_id) DO UPDATE
                SET ingest_bytes = license_usage_daily.ingest_bytes + EXCLUDED.ingest_bytes",
        )
        .bind(day)
        .bind(&org_id.0)
        .bind(bytes)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn get_ingest_bytes(&self, org_id: &Id, day: &str) -> Result<i64> {
        let row = sqlx::query(
            "SELECT ingest_bytes FROM license_usage_daily WHERE day = $1 AND org_id = $2",
        )
        .bind(day)
        .bind(&org_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(row
            .map(|r| r.try_get::<i64, _>("ingest_bytes").unwrap_or(0))
            .unwrap_or(0))
    }

    async fn add_hourly_ingest_bytes(
        &self,
        org_id: &Id,
        timestamp_micros: i64,
        bytes: i64,
    ) -> Result<()> {
        let bucket_start_micros = hour_bucket_start(timestamp_micros);
        sqlx::query(
            "INSERT INTO ingest_usage_hourly (org_id, bucket_start_micros, ingest_bytes)
             VALUES ($1, $2, $3)
             ON CONFLICT (org_id, bucket_start_micros) DO UPDATE
                SET ingest_bytes = ingest_usage_hourly.ingest_bytes + EXCLUDED.ingest_bytes",
        )
        .bind(&org_id.0)
        .bind(bucket_start_micros)
        .bind(bytes)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn hourly_ingest_bytes(
        &self,
        org_id: &Id,
        start_micros: i64,
        end_micros: i64,
    ) -> Result<Vec<IngestUsageBucket>> {
        let first_bucket = hour_bucket_start(start_micros);
        let rows = sqlx::query(
            "SELECT bucket_start_micros, ingest_bytes
             FROM ingest_usage_hourly
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
                Ok(IngestUsageBucket {
                    bucket_start_micros: row.try_get("bucket_start_micros").map_err(sqlx_err)?,
                    ingest_bytes: row.try_get("ingest_bytes").map_err(sqlx_err)?,
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
