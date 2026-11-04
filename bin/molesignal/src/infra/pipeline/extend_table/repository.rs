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
#[serde(rename_all = "camelCase")]
pub struct ExtendRow {
    pub id: Id,
    pub org_id: Id,
    pub table_name: String,
    pub key: String,
    pub value_json: Value,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtendValueField {
    pub name: String,
    pub field_type: String,
    pub required: bool,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
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
    async fn update_table_fields(
        &self,
        org: &Id,
        table: &str,
        value_fields: &[ExtendValueField],
        updated_at: TimestampMicros,
        require_empty: bool,
    ) -> Result<bool>;
    async fn delete_table(&self, org: &Id, table: &str) -> Result<()>;
    async fn upsert(&self, row: ExtendRow) -> Result<()>;

    /// Writes `rows`, replacing the rows already there under the same keys. A key given more than
    /// once keeps its last row.
    async fn upsert_rows(&self, rows: Vec<ExtendRow>) -> Result<()> {
        for row in latest_per_key(rows) {
            self.upsert(row).await?;
        }

        Ok(())
    }

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

const COLS: &str = "id, org_id, table_name, key, value_json, updated_at";

/// At most this many rows go into one statement of [`ExtendKvRepository::upsert_rows`].
const UPSERT_CHUNK: usize = 500;

/// The last row of every key, in the order of those last rows: one statement cannot write a key
/// twice.
fn latest_per_key(rows: Vec<ExtendRow>) -> Vec<ExtendRow> {
    let mut last: std::collections::HashMap<(String, String, String), usize> =
        std::collections::HashMap::new();

    for (index, row) in rows.iter().enumerate() {
        last.insert(
            (
                row.org_id.0.clone(),
                row.table_name.clone(),
                row.key.clone(),
            ),
            index,
        );
    }

    rows.into_iter()
        .enumerate()
        .filter(|(index, row)| {
            last.get(&(
                row.org_id.0.clone(),
                row.table_name.clone(),
                row.key.clone(),
            )) == Some(index)
        })
        .map(|(_, row)| row)
        .collect()
}

fn row_to(r: sqlx::postgres::PgRow) -> ExtendRow {
    let v: Json<Value> = r.try_get("value_json").unwrap_or(Json(Value::Null));

    ExtendRow {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        org_id: Id(r.try_get::<String, _>("org_id").unwrap_or_default()),
        table_name: r.try_get::<String, _>("table_name").unwrap_or_default(),
        key: r.try_get::<String, _>("key").unwrap_or_default(),
        value_json: v.0,
        updated_at: r
            .try_get::<TimestampMicros, _>("updated_at")
            .unwrap_or_default(),
    }
}

#[async_trait]
impl ExtendKvRepository for PgExtendKvRepository {
    async fn create_table(&self, table: ExtendTableDefinition) -> Result<ExtendTableDefinition> {
        sqlx::query(
            "INSERT INTO extend_table_definitions
                (org_id, table_name, description, key_field, value_fields_json,
                 created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&table.org_id.0)
        .bind(&table.table_name)
        .bind(&table.description)
        .bind(&table.key_field)
        .bind(Json(&table.value_fields))
        .bind(table.created_at)
        .bind(table.updated_at)
        .execute(&self.pool)
        .await
        .map_err(super::super::super::persistence::sqlx_err)?;

        Ok(table)
    }

    async fn update_table_fields(
        &self,
        org: &Id,
        table: &str,
        value_fields: &[ExtendValueField],
        updated_at: TimestampMicros,
        require_empty: bool,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE extend_table_definitions AS definitions
                SET value_fields_json = $3,
                    updated_at = $4
              WHERE definitions.org_id = $1
                AND definitions.table_name = $2
                AND (
                    NOT $5
                    OR NOT EXISTS (
                        SELECT 1
                          FROM extend_kv AS rows
                         WHERE rows.org_id = definitions.org_id
                           AND rows.table_name = definitions.table_name
                    )
                )",
        )
        .bind(&org.0)
        .bind(table)
        .bind(Json(value_fields))
        .bind(updated_at)
        .bind(require_empty)
        .execute(&self.pool)
        .await
        .map_err(super::super::super::persistence::sqlx_err)?;

        Ok(result.rows_affected() == 1)
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
                (id, org_id, table_name, key, value_json, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (org_id, table_name, key) DO UPDATE
             SET value_json = EXCLUDED.value_json,
                 updated_at = EXCLUDED.updated_at",
        )
        .bind(&row.id.0)
        .bind(&row.org_id.0)
        .bind(&row.table_name)
        .bind(&row.key)
        .bind(Json(&row.value_json))
        .bind(row.updated_at)
        .execute(&self.pool)
        .await
        .map_err(super::super::super::persistence::sqlx_err)?;

        Ok(())
    }

    async fn upsert_rows(&self, rows: Vec<ExtendRow>) -> Result<()> {
        for chunk in latest_per_key(rows).chunks(UPSERT_CHUNK) {
            let ids: Vec<&str> = chunk.iter().map(|row| row.id.0.as_str()).collect();
            let orgs: Vec<&str> = chunk.iter().map(|row| row.org_id.0.as_str()).collect();
            let tables: Vec<&str> = chunk.iter().map(|row| row.table_name.as_str()).collect();
            let keys: Vec<&str> = chunk.iter().map(|row| row.key.as_str()).collect();
            let values: Vec<String> = chunk.iter().map(|row| row.value_json.to_string()).collect();
            let times: Vec<TimestampMicros> = chunk.iter().map(|row| row.updated_at).collect();

            sqlx::query(
                "INSERT INTO extend_kv
                    (id, org_id, table_name, key, value_json, updated_at)
                 SELECT id, org_id, table_name, key, value_json::jsonb, updated_at
                   FROM UNNEST($1::text[], $2::text[], $3::text[], $4::text[], $5::text[], $6::timestamptz[])
                        AS rows (id, org_id, table_name, key, value_json, updated_at)
                 ON CONFLICT (org_id, table_name, key) DO UPDATE
                 SET value_json = EXCLUDED.value_json,
                     updated_at = EXCLUDED.updated_at",
            )
            .bind(&ids)
            .bind(&orgs)
            .bind(&tables)
            .bind(&keys)
            .bind(&values)
            .bind(&times)
            .execute(&self.pool)
            .await
            .map_err(super::super::super::persistence::sqlx_err)?;
        }

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
                    created_at, updated_at
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
                    MAX(rows.updated_at) AS updated_at,
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
                    updated_at: row
                        .try_get::<TimestampMicros, _>("updated_at")
                        .unwrap_or_default(),
                },
            );
        }

        for row in stats {
            let table_name = row.try_get::<String, _>("table_name").unwrap_or_default();
            let row_count = row.try_get::<i64, _>("row_count").unwrap_or_default();

            let updated_at = row
                .try_get::<TimestampMicros, _>("updated_at")
                .unwrap_or_default();

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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn row(table: &str, key: &str, value: i64) -> ExtendRow {
        ExtendRow {
            id: Id(format!("{table}-{key}-{value}")),
            org_id: Id("o1".into()),
            table_name: table.into(),
            key: key.into(),
            value_json: json!(value),
            updated_at: TimestampMicros(value),
        }
    }

    #[test]
    fn a_key_written_twice_keeps_its_last_row() {
        let rows = latest_per_key(vec![
            row("hosts", "a", 1),
            row("hosts", "b", 2),
            row("hosts", "a", 3),
            row("owners", "a", 4),
        ]);

        let kept: Vec<(&str, &str, &Value)> = rows
            .iter()
            .map(|row| (row.table_name.as_str(), row.key.as_str(), &row.value_json))
            .collect();

        assert_eq!(
            kept,
            vec![
                ("hosts", "b", &json!(2)),
                ("hosts", "a", &json!(3)),
                ("owners", "a", &json!(4)),
            ]
        );
    }
}
