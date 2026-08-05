// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `annotations` 表 Pg 实装（spec annotations capability）。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annotation {
    pub id: Id,
    pub org_id: Id,
    pub title: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub time_start: TimestampMicros,
    pub time_end: TimestampMicros,
    pub dashboard_id: Option<Id>,
    pub stream_name: Option<String>,
    pub created_by: Id,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone, Default)]
pub struct AnnotationFilter<'a> {
    pub dashboard_id: Option<&'a str>,
    pub stream_name: Option<&'a str>,
    pub tag: Option<&'a str>,
    pub from_micros: Option<i64>,
    pub to_micros: Option<i64>,
}

#[async_trait]
pub trait AnnotationRepository: Send + Sync {
    async fn create(&self, a: Annotation) -> Result<Annotation>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<Annotation>;
    async fn list(&self, org_id: &Id, f: AnnotationFilter<'_>) -> Result<Vec<Annotation>>;
    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()>;
}

pub struct PgAnnotationRepository {
    pool: PgPool,
}

impl PgAnnotationRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, title, description, tags, time_start_micros,
                    time_end_micros, dashboard_id, stream_name, created_by, created_at_micros";

fn row_to(r: sqlx::postgres::PgRow) -> Annotation {
    let tags: Json<Vec<String>> = r.try_get("tags").unwrap_or(Json(Vec::new()));
    Annotation {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        title: r.try_get::<String, _>("title").unwrap_or_default(),
        description: r
            .try_get::<Option<String>, _>("description")
            .unwrap_or_default(),
        tags: tags.0,
        time_start: TimestampMicros(r.try_get::<i64, _>("time_start_micros").unwrap_or_default()),
        time_end: TimestampMicros(r.try_get::<i64, _>("time_end_micros").unwrap_or_default()),
        dashboard_id: r
            .try_get::<Option<String>, _>("dashboard_id")
            .unwrap_or_default()
            .map(Id),
        stream_name: r
            .try_get::<Option<String>, _>("stream_name")
            .unwrap_or_default(),
        created_by: Id(r.try_get::<String, _>("created_by").unwrap_or_default()),
        created_at: TimestampMicros(r.try_get::<i64, _>("created_at_micros").unwrap_or_default()),
    }
}

#[async_trait]
impl AnnotationRepository for PgAnnotationRepository {
    async fn create(&self, a: Annotation) -> Result<Annotation> {
        sqlx::query(
            "INSERT INTO annotations
                (id, org_id, title, description, tags, time_start_micros, time_end_micros,
                 dashboard_id, stream_name, created_by, created_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(&a.id.0)
        .bind(&a.org_id.0)
        .bind(&a.title)
        .bind(&a.description)
        .bind(Json(&a.tags))
        .bind(a.time_start.0)
        .bind(a.time_end.0)
        .bind(a.dashboard_id.as_ref().map(|i| &i.0))
        .bind(&a.stream_name)
        .bind(&a.created_by.0)
        .bind(a.created_at.0)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(a)
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<Annotation> {
        let sql = format!("SELECT {COLS} FROM annotations WHERE org_id = $1 AND id = $2");
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(row_to(row))
    }

    async fn list(&self, org_id: &Id, f: AnnotationFilter<'_>) -> Result<Vec<Annotation>> {
        // 动态 WHERE 拼装；用 $1..$5 顺序绑定可选字段（NULL 时 OR TRUE 短路）。
        let sql = format!(
            "SELECT {COLS} FROM annotations
             WHERE org_id = $1
               AND ($2::TEXT IS NULL OR dashboard_id = $2)
               AND ($3::TEXT IS NULL OR stream_name = $3)
               AND ($4::BIGINT IS NULL OR time_start_micros >= $4)
               AND ($5::BIGINT IS NULL OR time_end_micros <= $5)
             ORDER BY time_start_micros DESC
             LIMIT 1000"
        );
        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(f.dashboard_id)
            .bind(f.stream_name)
            .bind(f.from_micros)
            .bind(f.to_micros)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        let mut out: Vec<Annotation> = rows.into_iter().map(row_to).collect();
        // tag 过滤在内存做（JSONB array 含元素 query 复杂，1000 行内可接受）
        if let Some(tag) = f.tag {
            out.retain(|a| a.tags.iter().any(|t| t == tag));
        }
        Ok(out)
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM annotations WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }
}

/// 反序列化辅助：JSON `Value::Array<String>` → `Vec<String>`，类型不匹配按空。
pub fn parse_tags(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parse_tags_handles_array_and_non_array() {
        assert_eq!(parse_tags(&json!(["a", "b"])), vec!["a", "b"]);
        assert_eq!(parse_tags(&json!("not array")), Vec::<String>::new());
        assert_eq!(parse_tags(&json!(null)), Vec::<String>::new());
    }
}
