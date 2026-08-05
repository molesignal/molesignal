// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `quotas` 表 Pg 实装 + 存储用量聚合。
//!
//! per-org 配额上限（`max_ingest_qps` / `max_storage_bytes` / ...）存 `quotas` 表；
//! 当前存储用量由 `parquet_file_meta.size_bytes` 按 org 求和得到。两者由后台 refresh loop
//! 周期载入进进程内 [`crate::infra::quotas::QuotaLimiter`]，摄取门禁据此做 429 / 413 判定。

use std::collections::HashMap;

use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::{
    infra::quotas::OrgQuota,
    shared::{Result, ids::Id},
};

pub struct PgQuotaRepository {
    pool: PgPool,
}

impl PgQuotaRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 载入所有 org 的配额上限（`quotas` 表）。无记录的 org 不在 map 中 → 视作无限制。
    pub async fn load_quotas(&self) -> Result<HashMap<Id, OrgQuota>> {
        let rows = sqlx::query(
            "SELECT org_id, max_ingest_qps, max_query_qps, max_storage_bytes, max_streams FROM quotas",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        let mut out = HashMap::with_capacity(rows.len());
        for r in rows {
            let org_id: String = r.try_get("org_id").map_err(sqlx_err)?;
            let max_ingest_qps: i32 = r.try_get("max_ingest_qps").unwrap_or(0);
            let max_query_qps: i32 = r.try_get("max_query_qps").unwrap_or(0);
            let max_storage_bytes: i64 = r.try_get("max_storage_bytes").unwrap_or(0);
            let max_streams: i32 = r.try_get("max_streams").unwrap_or(0);
            out.insert(
                Id::from_string(org_id),
                OrgQuota {
                    max_ingest_qps: max_ingest_qps.max(0) as u32,
                    max_query_qps: max_query_qps.max(0) as u32,
                    max_storage_bytes: max_storage_bytes.max(0) as u64,
                    max_streams: max_streams.max(0) as u32,
                },
            );
        }
        Ok(out)
    }

    /// 按 org 聚合当前存储字节（活跃 parquet 的 `parquet_file_meta.size_bytes` 之和）。
    pub async fn storage_usage(&self) -> Result<HashMap<Id, u64>> {
        let rows = sqlx::query(
            "SELECT org_id, COALESCE(SUM(size_bytes), 0)::BIGINT AS bytes \
             FROM parquet_file_meta WHERE deleted = false GROUP BY org_id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        let mut out = HashMap::with_capacity(rows.len());
        for r in rows {
            let org_id: String = r.try_get("org_id").map_err(sqlx_err)?;
            let bytes: i64 = r.try_get("bytes").unwrap_or(0);
            out.insert(Id::from_string(&org_id), bytes.max(0) as u64);
        }
        Ok(out)
    }
}
