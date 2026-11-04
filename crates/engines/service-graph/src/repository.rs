// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Where service graph edges are kept: `service_graph_edges`, a row per
//! caller, callee, environment and minute; read whole, summed over a window,
//! or a step at a time for one edge.

use async_trait::async_trait;
use kernel::time::TimestampMicros;
use postgres::sqlx_err;
use sqlx::{PgPool, Row};

use crate::{
    BUCKET_SIZE_US, EdgeSnapshot,
    shared::{Result, ids::Id},
};

#[async_trait]
pub trait ServiceGraphRepository: Send + Sync {
    async fn insert_many(&self, edges: &[EdgeSnapshot]) -> Result<()>;
    /// 删除某 org 在 `[from_micros, to_micros]`（按 bucket_at）内的边。
    /// storage 模式重算 worker 用来「先删后插」实现窗口幂等（重算覆盖、含晚到数据）。
    async fn delete_range(&self, org_id: &Id, from_micros: i64, to_micros: i64) -> Result<()>;
    async fn query(
        &self,
        org_id: &Id,
        from_micros: i64,
        to_micros: i64,
        service_filter: Option<&str>,
    ) -> Result<Vec<EdgeSnapshot>>;

    /// Each edge's calls over the window, in `environment` (every one when
    /// `None`): the counts summed, the percentiles those of its slowest minute.
    async fn summarize(
        &self,
        org_id: &Id,
        from_micros: i64,
        to_micros: i64,
        environment: Option<&str>,
    ) -> Result<Vec<EdgeSnapshot>>;

    /// One edge's calls a `step_micros` at a time, in `environment` (every
    /// one when `None`), the earliest first.
    #[allow(clippy::too_many_arguments)]
    async fn trend(
        &self,
        org_id: &Id,
        from_micros: i64,
        to_micros: i64,
        client_service: &str,
        server_service: &str,
        environment: Option<&str>,
        step_micros: i64,
    ) -> Result<Vec<EdgeSnapshot>>;
}

pub struct PgServiceGraphRepository {
    pool: PgPool,
}

impl PgServiceGraphRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ServiceGraphRepository for PgServiceGraphRepository {
    async fn delete_range(&self, org_id: &Id, from_micros: i64, to_micros: i64) -> Result<()> {
        sqlx::query(
            "DELETE FROM service_graph_edges
             WHERE org_id = $1 AND bucket_at BETWEEN $2 AND $3",
        )
        .bind(&org_id.0)
        .bind(TimestampMicros(from_micros))
        .bind(TimestampMicros(to_micros))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "service_graph_edges")
    )]
    async fn insert_many(&self, edges: &[EdgeSnapshot]) -> Result<()> {
        if edges.is_empty() {
            return Ok(());
        }

        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        for e in edges {
            sqlx::query(
                "INSERT INTO service_graph_edges
                    (id, org_id, client_service, server_service, environment, bucket_at,
                     request_count, error_count, p50_us, p95_us, p99_us)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
            )
            .bind(&e.id.0)
            .bind(&e.org_id.0)
            .bind(&e.client_service)
            .bind(&e.server_service)
            .bind(&e.environment)
            .bind(TimestampMicros(e.bucket_at))
            .bind(e.request_count as i64)
            .bind(e.error_count as i64)
            .bind(e.p50_us)
            .bind(e.p95_us)
            .bind(e.p99_us)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }

        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn query(
        &self,
        org_id: &Id,
        from_micros: i64,
        to_micros: i64,
        service_filter: Option<&str>,
    ) -> Result<Vec<EdgeSnapshot>> {
        let rows = if let Some(svc) = service_filter {
            sqlx::query(
                "SELECT id, org_id, client_service, server_service, environment, bucket_at,
                        request_count, error_count, p50_us, p95_us, p99_us
                 FROM service_graph_edges
                 WHERE org_id = $1 AND bucket_at BETWEEN $2 AND $3
                       AND (client_service = $4 OR server_service = $4)
                 ORDER BY bucket_at DESC
                 LIMIT 10000",
            )
            .bind(&org_id.0)
            .bind(TimestampMicros(from_micros))
            .bind(TimestampMicros(to_micros))
            .bind(svc)
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query(
                "SELECT id, org_id, client_service, server_service, environment, bucket_at,
                        request_count, error_count, p50_us, p95_us, p99_us
                 FROM service_graph_edges
                 WHERE org_id = $1 AND bucket_at BETWEEN $2 AND $3
                 ORDER BY bucket_at DESC
                 LIMIT 10000",
            )
            .bind(&org_id.0)
            .bind(TimestampMicros(from_micros))
            .bind(TimestampMicros(to_micros))
            .fetch_all(&self.pool)
            .await
        }
        .map_err(sqlx_err)?;

        Ok(rows.iter().map(edge_from_row).collect())
    }

    async fn summarize(
        &self,
        org_id: &Id,
        from_micros: i64,
        to_micros: i64,
        environment: Option<&str>,
    ) -> Result<Vec<EdgeSnapshot>> {
        let rows = sqlx::query(
            "SELECT '' AS id, org_id, client_service, server_service, environment,
                    MIN(bucket_at) AS bucket_at,
                    SUM(request_count)::BIGINT AS request_count,
                    SUM(error_count)::BIGINT AS error_count,
                    MAX(p50_us) AS p50_us, MAX(p95_us) AS p95_us, MAX(p99_us) AS p99_us
             FROM service_graph_edges
             WHERE org_id = $1 AND bucket_at BETWEEN $2 AND $3
                   AND ($4::TEXT IS NULL OR environment = $4)
             GROUP BY org_id, client_service, server_service, environment
             LIMIT 10000",
        )
        .bind(&org_id.0)
        .bind(TimestampMicros(from_micros))
        .bind(TimestampMicros(to_micros))
        .bind(environment)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(rows.iter().map(edge_from_row).collect())
    }

    async fn trend(
        &self,
        org_id: &Id,
        from_micros: i64,
        to_micros: i64,
        client_service: &str,
        server_service: &str,
        environment: Option<&str>,
        step_micros: i64,
    ) -> Result<Vec<EdgeSnapshot>> {
        let rows = sqlx::query(
            "SELECT '' AS id, org_id, client_service, server_service, '' AS environment,
                    date_bin($6 * INTERVAL '1 microsecond', bucket_at, TIMESTAMPTZ 'epoch') AS bucket_at,
                    SUM(request_count)::BIGINT AS request_count,
                    SUM(error_count)::BIGINT AS error_count,
                    MAX(p50_us) AS p50_us, MAX(p95_us) AS p95_us, MAX(p99_us) AS p99_us
             FROM service_graph_edges
             WHERE org_id = $1 AND bucket_at BETWEEN $2 AND $3
                   AND client_service = $4 AND server_service = $5
                   AND ($7::TEXT IS NULL OR environment = $7)
             GROUP BY org_id, client_service, server_service, 6
             ORDER BY 6",
        )
        .bind(&org_id.0)
        .bind(TimestampMicros(from_micros))
        .bind(TimestampMicros(to_micros))
        .bind(client_service)
        .bind(server_service)
        .bind(step_micros.max(BUCKET_SIZE_US))
        .bind(environment)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(rows.iter().map(edge_from_row).collect())
    }
}

fn edge_from_row(r: &sqlx::postgres::PgRow) -> EdgeSnapshot {
    EdgeSnapshot {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        client_service: r.try_get::<String, _>("client_service").unwrap_or_default(),
        server_service: r.try_get::<String, _>("server_service").unwrap_or_default(),
        environment: r.try_get::<String, _>("environment").unwrap_or_default(),
        bucket_at: r
            .try_get::<TimestampMicros, _>("bucket_at")
            .unwrap_or_default()
            .0,
        request_count: r.try_get::<i64, _>("request_count").unwrap_or_default() as u64,
        error_count: r.try_get::<i64, _>("error_count").unwrap_or_default() as u64,
        p50_us: r.try_get::<Option<i64>, _>("p50_us").unwrap_or_default(),
        p95_us: r.try_get::<Option<i64>, _>("p95_us").unwrap_or_default(),
        p99_us: r.try_get::<Option<i64>, _>("p99_us").unwrap_or_default(),
    }
}
