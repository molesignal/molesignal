// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `realtime_count_snapshots` 表：各节点实时规则窗口计数的快照，一个节点一条规则一行。

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::alerting::realtime_snapshot::{CountSnapshot, RealtimeSnapshotRepository},
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub struct PgRealtimeSnapshotRepository {
    pool: PgPool,
}

impl PgRealtimeSnapshotRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "node_id, rule_id, window_secs, buckets, saved_at";

fn row_to(row: sqlx::postgres::PgRow) -> Result<CountSnapshot> {
    let buckets: Json<Vec<(i64, u64)>> = row.try_get("buckets").map_err(sqlx_err)?;
    let window: i32 = row.try_get("window_secs").map_err(sqlx_err)?;

    Ok(CountSnapshot {
        node_id: row.try_get("node_id").map_err(sqlx_err)?,
        rule_id: Id(row.try_get("rule_id").map_err(sqlx_err)?),
        window_secs: u32::try_from(window).unwrap_or_default(),
        buckets: buckets.0,
        saved_at: row
            .try_get::<TimestampMicros, _>("saved_at")
            .map_err(sqlx_err)?,
    })
}

#[async_trait]
impl RealtimeSnapshotRepository for PgRealtimeSnapshotRepository {
    async fn replace_node(&self, node_id: &str, snapshots: &[CountSnapshot]) -> Result<()> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let kept: Vec<String> = snapshots
            .iter()
            .map(|snapshot| snapshot.rule_id.0.clone())
            .collect();

        sqlx::query(
            "DELETE FROM realtime_count_snapshots WHERE node_id = $1 AND NOT (rule_id = ANY($2))",
        )
        .bind(node_id)
        .bind(&kept)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        for snapshot in snapshots {
            sqlx::query(
                "INSERT INTO realtime_count_snapshots (node_id, rule_id, window_secs, buckets, saved_at)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (node_id, rule_id) DO UPDATE
                    SET window_secs = EXCLUDED.window_secs,
                        buckets = EXCLUDED.buckets,
                        saved_at = EXCLUDED.saved_at",
            )
            .bind(node_id)
            .bind(&snapshot.rule_id.0)
            .bind(i32::try_from(snapshot.window_secs).unwrap_or(i32::MAX))
            .bind(Json(&snapshot.buckets))
            .bind(snapshot.saved_at)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }

        tx.commit().await.map_err(sqlx_err)
    }

    async fn load_node(&self, node_id: &str) -> Result<Vec<CountSnapshot>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM realtime_count_snapshots WHERE node_id = $1"
        ))
        .bind(node_id)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to).collect()
    }

    async fn load_rules(&self, rule_ids: &[Id]) -> Result<Vec<CountSnapshot>> {
        let ids: Vec<String> = rule_ids.iter().map(|id| id.0.clone()).collect();
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM realtime_count_snapshots WHERE rule_id = ANY($1)"
        ))
        .bind(&ids)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to).collect()
    }

    async fn delete_before(&self, before: TimestampMicros) -> Result<u64> {
        let deleted = sqlx::query("DELETE FROM realtime_count_snapshots WHERE saved_at < $1")
            .bind(before)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(deleted.rows_affected())
    }
}
