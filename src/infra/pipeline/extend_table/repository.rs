// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `extend_kv` 表 CRUD。

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtendRow {
    pub id: Id,
    pub org_id: Id,
    pub table_name: String,
    pub key: String,
    pub value_json: Value,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtendValueField {
    pub name: String,
    pub field_type: String,
    pub required: bool,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtendTableDefinition {
    pub org_id: Id,
    pub table_name: String,
    pub description: String,
    pub key_field: String,
    pub value_fields: Vec<ExtendValueField>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtendTableSummary {
    pub table_name: String,
    pub description: String,
    pub key_field: String,
    pub value_fields: Vec<ExtendValueField>,
    pub row_count: i64,
    pub updated_at: TimestampMicros,
}

#[async_trait]
pub trait ExtendKvRepository: Send + Sync {
    async fn create_table(&self, table: ExtendTableDefinition) -> Result<ExtendTableDefinition>;
    async fn delete_table(&self, org: &Id, table: &str) -> Result<()>;
    async fn upsert(&self, row: ExtendRow) -> Result<()>;
    async fn delete(&self, org: &Id, table: &str, key: &str) -> Result<()>;
    async fn list_table(&self, org: &Id, table: &str) -> Result<Vec<ExtendRow>>;
    async fn list_tables(&self, org: &Id) -> Result<Vec<ExtendTableSummary>>;
}

pub struct PgExtendKvRepository {
    pool: PgPool,
}

impl PgExtendKvRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, org_id, table_name, key, value_json, updated_at_micros";

fn row_to(r: sqlx::postgres::PgRow) -> ExtendRow {
    let v: Json<Value> = r.try_get("value_json").unwrap_or(Json(Value::Null));
    ExtendRow {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        table_name: r.try_get::<String, _>("table_name").unwrap_or_default(),
        key: r.try_get::<String, _>("key").unwrap_or_default(),
        value_json: v.0,
        updated_at: TimestampMicros(r.try_get::<i64, _>("updated_at_micros").unwrap_or_default()),
    }
}

#[async_trait]
impl ExtendKvRepository for PgExtendKvRepository {
    async fn create_table(&self, table: ExtendTableDefinition) -> Result<ExtendTableDefinition> {
        sqlx::query(
            "INSERT INTO extend_table_definitions
                (org_id, table_name, description, key_field, value_fields_json,
                 created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&table.org_id.0)
        .bind(&table.table_name)
        .bind(&table.description)
        .bind(&table.key_field)
        .bind(Json(&table.value_fields))
        .bind(table.created_at.0)
        .bind(table.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(super::super::super::persistence::sqlx_err)?;
        Ok(table)
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "extend_tables")
    )]
    async fn delete_table(&self, org: &Id, table: &str) -> Result<()> {
        let mut tx = sqlx::begin(&self.pool)
            .await
            .map_err(super::super::super::persistence::sqlx_err)?;
        sqlx::query("DELETE FROM extend_kv WHERE org_id = $1 AND table_name = $2")
            .bind(&org.0)
            .bind(table)
            .execute(&mut *tx)
            .await
            .map_err(super::super::super::persistence::sqlx_err)?;
        sqlx::query("DELETE FROM extend_table_definitions WHERE org_id = $1 AND table_name = $2")
            .bind(&org.0)
            .bind(table)
            .execute(&mut *tx)
            .await
            .map_err(super::super::super::persistence::sqlx_err)?;
        tx.commit()
            .await
            .map_err(super::super::super::persistence::sqlx_err)?;
        Ok(())
    }

    async fn upsert(&self, row: ExtendRow) -> Result<()> {
        sqlx::query(
            "INSERT INTO extend_kv
                (id, org_id, table_name, key, value_json, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (org_id, table_name, key) DO UPDATE
             SET value_json = EXCLUDED.value_json,
                 updated_at_micros = EXCLUDED.updated_at_micros",
        )
        .bind(&row.id.0)
        .bind(&row.org_id.0)
        .bind(&row.table_name)
        .bind(&row.key)
        .bind(Json(&row.value_json))
        .bind(row.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(super::super::super::persistence::sqlx_err)?;
        Ok(())
    }

    async fn delete(&self, org: &Id, table: &str, key: &str) -> Result<()> {
        sqlx::query("DELETE FROM extend_kv WHERE org_id = $1 AND table_name = $2 AND key = $3")
            .bind(&org.0)
            .bind(table)
            .bind(key)
            .execute(&self.pool)
            .await
            .map_err(super::super::super::persistence::sqlx_err)?;
        Ok(())
    }

    async fn list_table(&self, org: &Id, table: &str) -> Result<Vec<ExtendRow>> {
        let sql = format!(
            "SELECT {COLS} FROM extend_kv WHERE org_id = $1 AND table_name = $2 ORDER BY key"
        );
        let rows = sqlx::query(&sql)
            .bind(&org.0)
            .bind(table)
            .fetch_all(&self.pool)
            .await
            .map_err(super::super::super::persistence::sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn list_tables(&self, org: &Id) -> Result<Vec<ExtendTableSummary>> {
        let definitions = sqlx::query(
            "SELECT table_name, description, key_field, value_fields_json,
                    created_at_micros, updated_at_micros
               FROM extend_table_definitions
              WHERE org_id = $1
              ORDER BY table_name",
        )
        .bind(&org.0)
        .fetch_all(&self.pool)
        .await
        .map_err(super::super::super::persistence::sqlx_err)?;
        let stats = sqlx::query(
            "SELECT rows.table_name,
                    COUNT(DISTINCT rows.key)::BIGINT AS row_count,
                    MAX(rows.updated_at_micros) AS updated_at_micros,
                    COALESCE(
                        ARRAY_AGG(DISTINCT fields.field_name)
                            FILTER (WHERE fields.field_name IS NOT NULL),
                        ARRAY[]::TEXT[]
                    ) AS inferred_fields
               FROM extend_kv AS rows
               LEFT JOIN LATERAL jsonb_object_keys(
                   CASE
                       WHEN jsonb_typeof(rows.value_json) = 'object' THEN rows.value_json
                       ELSE '{}'::JSONB
                   END
               ) AS fields(field_name) ON TRUE
              WHERE rows.org_id = $1
              GROUP BY rows.table_name
              ORDER BY rows.table_name",
        )
        .bind(&org.0)
        .fetch_all(&self.pool)
        .await
        .map_err(super::super::super::persistence::sqlx_err)?;

        let mut summaries = BTreeMap::<String, ExtendTableSummary>::new();
        for row in definitions {
            let table_name = row.try_get::<String, _>("table_name").unwrap_or_default();
            let fields = row
                .try_get::<Json<Vec<ExtendValueField>>, _>("value_fields_json")
                .map(|value| value.0)
                .unwrap_or_default();
            summaries.insert(
                table_name.clone(),
                ExtendTableSummary {
                    table_name,
                    description: row.try_get::<String, _>("description").unwrap_or_default(),
                    key_field: row
                        .try_get::<String, _>("key_field")
                        .unwrap_or_else(|_| "key".to_string()),
                    value_fields: fields,
                    row_count: 0,
                    updated_at: TimestampMicros(
                        row.try_get::<i64, _>("updated_at_micros")
                            .unwrap_or_default(),
                    ),
                },
            );
        }

        for row in stats {
            let table_name = row.try_get::<String, _>("table_name").unwrap_or_default();
            let row_count = row.try_get::<i64, _>("row_count").unwrap_or_default();
            let updated_at = TimestampMicros(
                row.try_get::<i64, _>("updated_at_micros")
                    .unwrap_or_default(),
            );
            let inferred_fields = row
                .try_get::<Vec<String>, _>("inferred_fields")
                .unwrap_or_default()
                .into_iter()
                .map(|name| ExtendValueField {
                    name,
                    field_type: "string".to_string(),
                    required: false,
                    description: String::new(),
                })
                .collect::<Vec<_>>();
            summaries
                .entry(table_name.clone())
                .and_modify(|summary| {
                    summary.row_count = row_count;
                    summary.updated_at = TimestampMicros(summary.updated_at.0.max(updated_at.0));
                    if summary.value_fields.is_empty() {
                        summary.value_fields.clone_from(&inferred_fields);
                    }
                })
                .or_insert_with(|| ExtendTableSummary {
                    table_name,
                    description: String::new(),
                    key_field: "key".to_string(),
                    value_fields: inferred_fields,
                    row_count,
                    updated_at,
                });
        }
        Ok(summaries.into_values().collect())
    }
}
