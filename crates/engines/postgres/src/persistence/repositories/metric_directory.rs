// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `metric_directory` 表的 Pg 实装：每个指标流里有哪些指标、见于哪些小时、带哪些标签名。
//!
//! 登记是一条 `INSERT .. ON CONFLICT DO UPDATE`：已知的指标只放宽小时范围、并入新标签名，
//! 多个 intake 节点并发登记同一行也不需要额外加锁。保留期删掉数据后，compactor 让目录也忘掉
//! 截止时间之前的部分。

use async_trait::async_trait;
use serde_json::json;
use sqlx::{PgPool, Row, postgres::PgRow, types::Json};

use super::sqlx_err;
use crate::{
    domain::{
        metrics::{MetricDirectory, MetricDirectoryEntry, MetricObservation},
        stream::StreamDefinition,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

const ENTRY_COLUMNS: &str = "d.stream_id, s.name AS stream_name, d.metric_name, d.metric_kind, \
                             d.labels, d.first_seen, d.last_seen";

pub struct PgMetricDirectory {
    pool: PgPool,
}

impl PgMetricDirectory {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MetricDirectory for PgMetricDirectory {
    async fn record(
        &self,
        stream: &StreamDefinition,
        observations: &[MetricObservation],
    ) -> Result<()> {
        if observations.is_empty() {
            return Ok(());
        }

        let rows = observations
            .iter()
            .map(|observation| {
                json!({
                    "metric_name": observation.metric_name,
                    "metric_kind": observation.metric_kind,
                    "labels": observation.labels,
                    "first_seen": observation.first_seen.0,
                    "last_seen": observation.last_seen.0,
                })
            })
            .collect::<Vec<_>>();

        sqlx::query(
            "INSERT INTO metric_directory (
                org_id, stream_id, metric_name, metric_kind, labels,
                first_seen, last_seen
             )
             SELECT $1, $2, o.metric_name, o.metric_kind, o.labels,
                    TIMESTAMPTZ 'epoch' + o.first_seen * INTERVAL '1 microsecond',
                    TIMESTAMPTZ 'epoch' + o.last_seen * INTERVAL '1 microsecond'
             FROM jsonb_to_recordset($3::JSONB) AS o(
                metric_name TEXT, metric_kind TEXT, labels JSONB,
                first_seen BIGINT, last_seen BIGINT
             )
             ON CONFLICT (org_id, stream_id, metric_name) DO UPDATE SET
                metric_kind = COALESCE(EXCLUDED.metric_kind, metric_directory.metric_kind),
                labels = (
                    SELECT COALESCE(jsonb_agg(DISTINCT label ORDER BY label), '[]'::JSONB)
                    FROM jsonb_array_elements_text(metric_directory.labels || EXCLUDED.labels)
                         AS label
                ),
                first_seen = LEAST(
                    metric_directory.first_seen, EXCLUDED.first_seen
                ),
                last_seen = GREATEST(
                    metric_directory.last_seen, EXCLUDED.last_seen
                )",
        )
        .bind(&stream.org_id.0)
        .bind(&stream.id.0)
        .bind(Json(serde_json::Value::Array(rows)))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<MetricDirectoryEntry>> {
        let rows = sqlx::query(&format!(
            "SELECT {ENTRY_COLUMNS}
             FROM metric_directory d
             JOIN logical_streams s ON s.org_id = d.org_id AND s.id = d.stream_id
             WHERE d.org_id = $1
             ORDER BY d.metric_name, s.name"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter().map(entry).collect()
    }

    async fn find(&self, org_id: &Id, metric_name: &str) -> Result<Vec<MetricDirectoryEntry>> {
        let rows = sqlx::query(&format!(
            "SELECT {ENTRY_COLUMNS}
             FROM metric_directory d
             JOIN logical_streams s ON s.org_id = d.org_id AND s.id = d.stream_id
             WHERE d.org_id = $1 AND d.metric_name = $2
             ORDER BY s.name"
        ))
        .bind(&org_id.0)
        .bind(metric_name)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter().map(entry).collect()
    }

    async fn forget_before(
        &self,
        stream: &StreamDefinition,
        cutoff: TimestampMicros,
    ) -> Result<u64> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        let dropped = sqlx::query(
            "DELETE FROM metric_directory
             WHERE org_id = $1 AND stream_id = $2 AND last_seen <= $3",
        )
        .bind(&stream.org_id.0)
        .bind(&stream.id.0)
        .bind(cutoff)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?
        .rows_affected();

        sqlx::query(
            "UPDATE metric_directory SET first_seen = $3
             WHERE org_id = $1 AND stream_id = $2 AND first_seen < $3",
        )
        .bind(&stream.org_id.0)
        .bind(&stream.id.0)
        .bind(cutoff)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(dropped)
    }
}

fn entry(row: &PgRow) -> Result<MetricDirectoryEntry> {
    let labels: Json<Vec<String>> = row.try_get("labels").map_err(sqlx_err)?;

    Ok(MetricDirectoryEntry {
        stream_id: Id(row.try_get("stream_id").map_err(sqlx_err)?),
        stream_name: row.try_get("stream_name").map_err(sqlx_err)?,
        metric_name: row.try_get("metric_name").map_err(sqlx_err)?,
        metric_kind: row.try_get("metric_kind").map_err(sqlx_err)?,
        labels: labels.0,
        first_seen: row
            .try_get::<TimestampMicros, _>("first_seen")
            .map_err(sqlx_err)?,
        last_seen: row
            .try_get::<TimestampMicros, _>("last_seen")
            .map_err(sqlx_err)?,
    })
}
