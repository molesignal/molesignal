// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ⌘K 远端聚合搜索仓库（web-investigation-shell）。
//!
//! 在 streams / dashboards / saved_views / alert_rules / incidents /
//! service_graph_edges 6 张表上跑 UNION ALL，按 pg_trgm `similarity` 排序。
//! 每张表都要求 `gin_trgm_ops` 索引就绪（见初始 schema）。

use async_trait::async_trait;
use serde::Serialize;
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::shared::{Result, ids::Id};

#[derive(Debug, Clone, Serialize)]
pub struct WebSearchHit {
    pub kind: &'static str,
    pub id: String,
    pub label: String,
    pub subtitle: Option<String>,
    pub rank: f32,
}

#[async_trait]
pub trait WebSearchRepository: Send + Sync {
    /// `kinds` 为空表示全部 6 类；非空仅包含指定 kind。
    async fn search(
        &self,
        org_id: &Id,
        q: &str,
        kinds: &[&str],
        limit: u32,
    ) -> Result<Vec<WebSearchHit>>;
}

pub struct PgWebSearchRepository {
    pool: PgPool,
}

impl PgWebSearchRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn list_stream_hits(&self, org_id: &Id, limit: u32) -> Result<Vec<WebSearchHit>> {
        let limit = limit.clamp(1, 50) as i64;
        let rows = sqlx::query(
            "SELECT id::text AS id, name AS label, stream_type AS subtitle
               FROM streams
              WHERE org_id = $1
              ORDER BY name ASC
              LIMIT $2",
        )
        .bind(&org_id.0)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(rows
            .into_iter()
            .map(|r| WebSearchHit {
                kind: "stream",
                id: r.try_get("id").unwrap_or_default(),
                label: r.try_get("label").unwrap_or_default(),
                subtitle: r.try_get::<Option<String>, _>("subtitle").unwrap_or(None),
                rank: 0.0,
            })
            .collect())
    }
}

/// 单表 SELECT 模板。`KIND` 列硬编码以便 UNION ALL 后区分。
struct KindQuery {
    kind: &'static str,
    sql: &'static str,
}

const ALL: &[KindQuery] = &[
    KindQuery {
        kind: "stream",
        sql: "SELECT 'stream' AS kind, id::text AS id, name AS label,
                     stream_type AS subtitle, similarity(name, $2) AS rk
              FROM streams WHERE org_id = $1 AND name % $2",
    },
    KindQuery {
        kind: "dashboard",
        sql: "SELECT 'dashboard' AS kind, id::text AS id, title AS label,
                     uid AS subtitle, similarity(title, $2) AS rk
              FROM dashboards WHERE org_id = $1 AND title % $2",
    },
    KindQuery {
        kind: "saved_view",
        sql: "SELECT 'saved_view' AS kind, id::text AS id, name AS label,
                     language AS subtitle, similarity(name, $2) AS rk
              FROM saved_views WHERE org_id = $1 AND name % $2",
    },
    KindQuery {
        kind: "alert",
        sql: "SELECT 'alert' AS kind, id::text AS id, name AS label,
                     CASE WHEN enabled THEN 'enabled' ELSE 'disabled' END AS subtitle,
                     similarity(name, $2) AS rk
              FROM alert_rules WHERE org_id = $1 AND name % $2",
    },
    KindQuery {
        kind: "incident",
        sql: "SELECT 'incident' AS kind, id::text AS id, fingerprint AS label,
                     status AS subtitle, similarity(fingerprint, $2) AS rk
              FROM incidents WHERE org_id = $1 AND fingerprint % $2",
    },
    KindQuery {
        kind: "service",
        sql: "SELECT 'service' AS kind, MIN(id::text) AS id, name AS label,
                     NULL::text AS subtitle, MAX(similarity(name, $2)) AS rk
              FROM (
                SELECT id, client_service AS name FROM service_graph_edges
                  WHERE org_id = $1 AND client_service % $2
                UNION ALL
                SELECT id, server_service AS name FROM service_graph_edges
                  WHERE org_id = $1 AND server_service % $2
              ) s GROUP BY name",
    },
];

#[async_trait]
impl WebSearchRepository for PgWebSearchRepository {
    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "web_search")
    )]
    async fn search(
        &self,
        org_id: &Id,
        q: &str,
        kinds: &[&str],
        limit: u32,
    ) -> Result<Vec<WebSearchHit>> {
        if q.trim().is_empty() {
            if kinds.contains(&"stream") {
                return self.list_stream_hits(org_id, limit).await;
            }
            return Ok(Vec::new());
        }
        let limit = limit.clamp(1, 50) as i64;

        let parts: Vec<&str> = ALL
            .iter()
            .filter(|kq| kinds.is_empty() || kinds.contains(&kq.kind))
            .map(|kq| kq.sql)
            .collect();
        if parts.is_empty() {
            return Ok(Vec::new());
        }

        let union = parts.join(" UNION ALL ");
        let sql = format!(
            "WITH hits AS ({union})
             SELECT kind, id, label, subtitle, rk
             FROM hits
             ORDER BY rk DESC NULLS LAST
             LIMIT $3"
        );

        // 200ms statement timeout。必须开显式事务：`SET LOCAL` 的作用域是事务块，
        // 在裸连接上执行完当场就失效，下面那条 6 表 UNION 会完全不受保护地跑下去。
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        sqlx::query("SET LOCAL statement_timeout = 200")
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;

        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(q)
            .bind(limit)
            .fetch_all(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        // 只读事务，commit 与 rollback 等价；commit 让连接干净地归还连接池。
        tx.commit().await.map_err(sqlx_err)?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let kind: String = r.try_get("kind").unwrap_or_default();
            let kind_static: &'static str = match kind.as_str() {
                "stream" => "stream",
                "dashboard" => "dashboard",
                "saved_view" => "saved_view",
                "alert" => "alert",
                "incident" => "incident",
                "service" => "service",
                _ => continue,
            };
            out.push(WebSearchHit {
                kind: kind_static,
                id: r.try_get("id").unwrap_or_default(),
                label: r.try_get("label").unwrap_or_default(),
                subtitle: r.try_get::<Option<String>, _>("subtitle").unwrap_or(None),
                rank: r.try_get::<f32, _>("rk").unwrap_or(0.0),
            });
        }
        Ok(out)
    }
}
