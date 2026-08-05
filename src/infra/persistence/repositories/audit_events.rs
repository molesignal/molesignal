// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `audit_events` 表 Pg 实装。
//!
//! 用于 IAM deny 路径 + 后续 ingest/query/管控类操作写审计。

use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::shared::{
    Result, ids::Id, time::TimestampMicros, trace_normalization::sanitize_telemetry_fields,
};

#[derive(Debug, Clone, Serialize)]
pub struct AuditEvent {
    pub id: Id,
    pub org_id: Id,
    pub actor_kind: String,
    pub actor_id: String,
    pub action: String,
    pub target_kind: Option<String>,
    pub target_id: Option<String>,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub payload: Value,
    pub ts: TimestampMicros,
}

/// 审计查询过滤条件（change `add-ai-anomaly-chat`，task 4.1）。
/// 所有过滤项可空；`cursor` 为上一页最后一行的 `(ts_micros, id)`，用于稳定游标分页。
#[derive(Debug, Clone, Default)]
pub struct AuditQuery {
    pub from_micros: Option<i64>,
    pub to_micros: Option<i64>,
    pub actor_kind: Option<String>,
    pub actor_id: Option<String>,
    pub action: Option<String>,
    pub target_kind: Option<String>,
    pub target_id: Option<String>,
    /// SQL LIMIT；调用方一般传 page_size + 1 以探测是否有下一页。
    pub limit: i64,
    /// `(ts_micros, id)`：仅返回 DESC 序中严格位于其后的行。
    pub cursor: Option<(i64, String)>,
}

#[async_trait]
pub trait AuditEventRepository: Send + Sync {
    async fn record(&self, e: AuditEvent) -> Result<()>;
    async fn list_recent(&self, org_id: &Id, limit: i64) -> Result<Vec<AuditEvent>>;
    /// 过滤 + 游标分页查询，按 `ts_micros DESC, id DESC` 排序。
    async fn query(&self, org_id: &Id, q: &AuditQuery) -> Result<Vec<AuditEvent>>;
}

pub struct PgAuditEventRepository {
    pool: PgPool,
}

impl PgAuditEventRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, actor_kind, actor_id, action, target_kind, target_id, ip, user_agent, payload, ts_micros";

fn row_to(row: sqlx::postgres::PgRow) -> Result<AuditEvent> {
    let payload: Json<Value> = row.try_get("payload").map_err(sqlx_err)?;
    Ok(AuditEvent {
        id: Id(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        actor_kind: row.try_get("actor_kind").map_err(sqlx_err)?,
        actor_id: row.try_get("actor_id").map_err(sqlx_err)?,
        action: row.try_get("action").map_err(sqlx_err)?,
        target_kind: row.try_get("target_kind").map_err(sqlx_err)?,
        target_id: row.try_get("target_id").map_err(sqlx_err)?,
        ip: row.try_get("ip").map_err(sqlx_err)?,
        user_agent: row.try_get("user_agent").map_err(sqlx_err)?,
        payload: payload.0,
        ts: TimestampMicros(row.try_get("ts_micros").map_err(sqlx_err)?),
    })
}

fn sanitize_audit_event(event: &mut AuditEvent) {
    let mut fields = Map::new();
    fields.insert("payload".into(), std::mem::take(&mut event.payload));
    if let Some(ip) = event.ip.take() {
        fields.insert("ip".into(), Value::String(ip));
    }
    if let Some(user_agent) = event.user_agent.take() {
        fields.insert("user_agent".into(), Value::String(user_agent));
    }
    sanitize_telemetry_fields(&mut fields, 4 * 1024);
    event.payload = fields.remove("payload").unwrap_or(Value::Null);
    event.ip = fields
        .remove("ip")
        .and_then(|value| value.as_str().map(str::to_owned));
    event.user_agent = fields
        .remove("user_agent")
        .and_then(|value| value.as_str().map(str::to_owned));
}

#[async_trait]
impl AuditEventRepository for PgAuditEventRepository {
    async fn record(&self, mut e: AuditEvent) -> Result<()> {
        // 审计是最后一道持久化边界；即使调用方误把 credential 放进 payload，
        // 原值也不能进入 `_sys` 或后续配置 diff 查询。
        sanitize_audit_event(&mut e);
        sqlx::query(
            "INSERT INTO audit_events
                (id, org_id, actor_kind, actor_id, action, target_kind, target_id,
                 ip, user_agent, payload, ts_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(&e.id.0)
        .bind(&e.org_id.0)
        .bind(&e.actor_kind)
        .bind(&e.actor_id)
        .bind(&e.action)
        .bind(&e.target_kind)
        .bind(&e.target_id)
        .bind(&e.ip)
        .bind(&e.user_agent)
        .bind(Json(&e.payload))
        .bind(e.ts.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn list_recent(&self, org_id: &Id, limit: i64) -> Result<Vec<AuditEvent>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM audit_events WHERE org_id = $1 ORDER BY ts_micros DESC LIMIT $2"
        ))
        .bind(&org_id.0)
        .bind(limit.clamp(1, 100))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to).collect()
    }

    async fn query(&self, org_id: &Id, q: &AuditQuery) -> Result<Vec<AuditEvent>> {
        // 全过滤项以 `($n IS NULL OR col = $n)` 形式恒绑定，避免动态 SQL 拼接。
        // 游标条件 `(ts_micros, id) < (cts, cid)` 在 DESC 序里取「下一页」。
        let (cursor_ts, cursor_id) = match &q.cursor {
            Some((ts, id)) => (Some(*ts), Some(id.clone())),
            None => (None, None),
        };
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM audit_events
             WHERE org_id = $1
               AND ($2::bigint IS NULL OR ts_micros >= $2)
               AND ($3::bigint IS NULL OR ts_micros <= $3)
               AND ($4::text   IS NULL OR actor_kind = $4)
               AND ($5::text   IS NULL OR actor_id = $5)
               AND ($6::text   IS NULL OR action = $6)
               AND ($7::text   IS NULL OR target_kind = $7)
               AND ($8::text   IS NULL OR target_id = $8)
               AND ($9::bigint IS NULL OR ts_micros < $9 OR (ts_micros = $9 AND id < $10))
             ORDER BY ts_micros DESC, id DESC
             LIMIT $11"
        ))
        .bind(&org_id.0)
        .bind(q.from_micros)
        .bind(q.to_micros)
        .bind(&q.actor_kind)
        .bind(&q.actor_id)
        .bind(&q.action)
        .bind(&q.target_kind)
        .bind(&q.target_id)
        .bind(cursor_ts)
        .bind(cursor_id)
        .bind(q.limit.clamp(1, 500))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to).collect()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn audit_persistence_boundary_removes_credentials_recursively() {
        let mut event = AuditEvent {
            id: Id("event".into()),
            org_id: Id("_sys".into()),
            actor_kind: "user".into(),
            actor_id: "opaque-user-id".into(),
            action: "trace_policy.update".into(),
            target_kind: None,
            target_id: None,
            ip: Some("127.0.0.1".into()),
            user_agent: Some("agent Bearer super-secret-token".into()),
            payload: json!({
                "enabled": true,
                "nested": {
                    "authorization": "Bearer super-secret-token",
                    "message": "contact alice@example.com"
                }
            }),
            ts: TimestampMicros(1),
        };

        sanitize_audit_event(&mut event);

        let encoded = serde_json::to_string(&event).unwrap();
        assert!(!encoded.contains("super-secret-token"));
        assert!(!encoded.contains("alice@example.com"));
        assert!(event.payload["enabled"].as_bool().unwrap());
        assert!(event.payload["nested"].get("authorization").is_none());
        assert_eq!(event.payload["nested"]["message"], "[REDACTED]");
        assert_eq!(event.user_agent.as_deref(), Some("[REDACTED]"));
    }
}
