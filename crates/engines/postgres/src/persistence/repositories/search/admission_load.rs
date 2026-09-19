// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `search_admission_load` 表 Pg 实装：跨节点 search admission 软协调。
//!
//! 每个 querier 节点周期 `publish` 其 per-work-group 并发 in_flight；其它节点
//! `cluster_totals_excluding` 读「集群（除自身）总和」，喂给本地 AdmissionController
//! 做软上限。最终一致——实时性受发布间隔约束，陈旧行按 `since` 过滤。

use std::collections::HashMap;

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::super::sqlx_err;
use crate::shared::{Result, time::TimestampMicros};

#[async_trait]
pub trait SearchAdmissionLoadRepository: Send + Sync {
    /// 发布本节点各工作组的当前 in_flight（按 (node, group) upsert，刷新 updated_at）。
    async fn publish(
        &self,
        node_id: &str,
        loads: &[(String, i64)],
        now: TimestampMicros,
    ) -> Result<()>;
    /// 集群中**除本节点外**、`updated_at >= since` 的各工作组 in_flight 总和。
    async fn cluster_totals_excluding(
        &self,
        node_id: &str,
        since: TimestampMicros,
    ) -> Result<HashMap<String, i64>>;
}

pub struct PgSearchAdmissionLoadRepository {
    pool: PgPool,
}

impl PgSearchAdmissionLoadRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SearchAdmissionLoadRepository for PgSearchAdmissionLoadRepository {
    async fn publish(
        &self,
        node_id: &str,
        loads: &[(String, i64)],
        now: TimestampMicros,
    ) -> Result<()> {
        for (group, in_flight) in loads {
            sqlx::query(
                "INSERT INTO search_admission_load (node_id, work_group, in_flight, updated_at_micros)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (node_id, work_group) DO UPDATE SET
                    in_flight = EXCLUDED.in_flight,
                    updated_at_micros = EXCLUDED.updated_at_micros",
            )
            .bind(node_id)
            .bind(group)
            .bind(*in_flight)
            .bind(now.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        }
        Ok(())
    }

    async fn cluster_totals_excluding(
        &self,
        node_id: &str,
        since: TimestampMicros,
    ) -> Result<HashMap<String, i64>> {
        let rows = sqlx::query(
            "SELECT work_group, SUM(in_flight) AS total
             FROM search_admission_load
             WHERE node_id <> $1 AND updated_at_micros >= $2
             GROUP BY work_group",
        )
        .bind(node_id)
        .bind(since.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        let mut out = HashMap::new();
        for r in rows {
            let group: String = r.try_get("work_group").map_err(sqlx_err)?;
            let total: i64 = r.try_get("total").map_err(sqlx_err)?;
            out.insert(group, total);
        }
        Ok(out)
    }
}
