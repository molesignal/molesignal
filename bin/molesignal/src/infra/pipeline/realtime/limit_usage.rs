// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `pipeline_limit_usage`：配额与限流步骤每个 UTC 日、每个桶放行了多少、超出了多少。各节点每隔
//! 几秒把自己新数的加上去（同一行的加法在库里做，几个节点同时写也不会丢），再读回所有节点的合计。
use async_trait::async_trait;
use sqlx::{PgPool, Row, postgres::PgRow};

use crate::{
    domain::pipeline::limit::{LimitKey, LimitUsage, LimitUsageLedger, LimitUsageRow},
    infra::persistence::sqlx_err,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 一条语句最多写这么多行。
const CHUNK: usize = 1_000;

const COLUMNS: &str = "org_id, pipeline_id, step, day_start, bucket, admitted_events, \
                       admitted_bytes, over_events, over_bytes";

pub struct PgLimitUsageLedger {
    pool: PgPool,
}

impl PgLimitUsageLedger {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl LimitUsageLedger for PgLimitUsageLedger {
    async fn add(&self, rows: &[LimitUsageRow], at: TimestampMicros) -> Result<()> {
        for chunk in rows.chunks(CHUNK) {
            let orgs: Vec<&str> = chunk.iter().map(|row| row.org_id.0.as_str()).collect();
            let pipelines: Vec<&str> = chunk.iter().map(|r| r.key.pipeline_id.0.as_str()).collect();
            let steps: Vec<&str> = chunk.iter().map(|row| row.key.step.as_str()).collect();
            let days: Vec<TimestampMicros> = chunk
                .iter()
                .map(|row| TimestampMicros(row.key.day))
                .collect();
            let buckets: Vec<&str> = chunk.iter().map(|row| row.bucket.as_str()).collect();
            let count = |pick: fn(&LimitUsage) -> u64| -> Vec<i64> {
                chunk
                    .iter()
                    .map(|row| i64::try_from(pick(&row.usage)).unwrap_or(i64::MAX))
                    .collect()
            };

            sqlx::query(
                "INSERT INTO pipeline_limit_usage
                    (org_id, pipeline_id, step, day_start, bucket, admitted_events,
                     admitted_bytes, over_events, over_bytes, updated_at)
                 SELECT org_id, pipeline_id, step, day_start, bucket, admitted_events,
                        admitted_bytes, over_events, over_bytes, $10
                   FROM UNNEST($1::text[], $2::text[], $3::text[], $4::timestamptz[], $5::text[],
                               $6::int8[], $7::int8[], $8::int8[], $9::int8[])
                        AS rows (org_id, pipeline_id, step, day_start, bucket,
                                 admitted_events, admitted_bytes, over_events, over_bytes)
                 ON CONFLICT (pipeline_id, step, day_start, bucket) DO UPDATE
                 SET admitted_events = pipeline_limit_usage.admitted_events + EXCLUDED.admitted_events,
                     admitted_bytes = pipeline_limit_usage.admitted_bytes + EXCLUDED.admitted_bytes,
                     over_events = pipeline_limit_usage.over_events + EXCLUDED.over_events,
                     over_bytes = pipeline_limit_usage.over_bytes + EXCLUDED.over_bytes,
                     updated_at = EXCLUDED.updated_at",
            )
            .bind(&orgs)
            .bind(&pipelines)
            .bind(&steps)
            .bind(&days)
            .bind(&buckets)
            .bind(count(|usage| usage.admitted_events))
            .bind(count(|usage| usage.admitted_bytes))
            .bind(count(|usage| usage.over_events))
            .bind(count(|usage| usage.over_bytes))
            .bind(at)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        }

        Ok(())
    }

    async fn totals(&self, keys: &[LimitKey]) -> Result<Vec<LimitUsageRow>> {
        let mut rows = Vec::new();

        for chunk in keys.chunks(CHUNK) {
            let pipelines: Vec<&str> = chunk.iter().map(|k| k.pipeline_id.0.as_str()).collect();
            let steps: Vec<&str> = chunk.iter().map(|key| key.step.as_str()).collect();
            let days: Vec<TimestampMicros> =
                chunk.iter().map(|key| TimestampMicros(key.day)).collect();

            let found = sqlx::query(&format!(
                "SELECT {COLUMNS} FROM pipeline_limit_usage
                  WHERE (pipeline_id, step, day_start) IN
                        (SELECT * FROM UNNEST($1::text[], $2::text[], $3::timestamptz[]))"
            ))
            .bind(&pipelines)
            .bind(&steps)
            .bind(&days)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;

            for row in found {
                rows.push(usage_row(&row)?);
            }
        }

        Ok(rows)
    }

    async fn of_pipeline(&self, org: &Id, pipeline: &Id, day: i64) -> Result<Vec<LimitUsageRow>> {
        sqlx::query(&format!(
            "SELECT {COLUMNS} FROM pipeline_limit_usage
              WHERE org_id = $1 AND pipeline_id = $2 AND day_start = $3
              ORDER BY step, bucket"
        ))
        .bind(&org.0)
        .bind(&pipeline.0)
        .bind(TimestampMicros(day))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?
        .iter()
        .map(usage_row)
        .collect()
    }

    async fn forget_before(&self, day: i64) -> Result<u64> {
        let done = sqlx::query("DELETE FROM pipeline_limit_usage WHERE day_start < $1")
            .bind(TimestampMicros(day))
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(done.rows_affected())
    }
}

fn usage_row(row: &PgRow) -> Result<LimitUsageRow> {
    let count = |name: &str| -> Result<u64> {
        let value: i64 = row.try_get(name).map_err(sqlx_err)?;
        u64::try_from(value).map_err(|_| Error::internal(format!("negative `{name}` stored")))
    };

    Ok(LimitUsageRow {
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        key: LimitKey {
            pipeline_id: Id(row.try_get("pipeline_id").map_err(sqlx_err)?),
            step: row.try_get("step").map_err(sqlx_err)?,
            day: row
                .try_get::<TimestampMicros, _>("day_start")
                .map_err(sqlx_err)?
                .0,
        },
        bucket: row.try_get("bucket").map_err(sqlx_err)?,
        usage: LimitUsage {
            admitted_events: count("admitted_events")?,
            admitted_bytes: count("admitted_bytes")?,
            over_events: count("over_events")?,
            over_bytes: count("over_bytes")?,
        },
    })
}
