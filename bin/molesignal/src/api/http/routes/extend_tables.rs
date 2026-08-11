// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Extend table CRUD over `extend_kv`.

use std::collections::{HashMap, HashSet};

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{iam::permission, saved_view::SavedView},
    infra::pipeline::{
        ExtendRow, ExtendTableDefinition, ExtendTableSummary, ExtendValueField, ScheduledPipeline,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/extend_tables", get(list_tables).post(create_table))
        .route(
            "/extend_tables/{table}",
            get(list_rows).delete(delete_table),
        )
        .route(
            "/extend_tables/{table}/rows/{key}",
            axum::routing::put(upsert_row).delete(delete_row),
        )
}

#[derive(Debug, Serialize)]
struct RowResp {
    id: String,
    org_id: String,
    table_name: String,
    key: String,
    value_json: Value,
    updated_at_micros: i64,
}

fn row_resp(row: ExtendRow) -> RowResp {
    RowResp {
        id: row.id.0,
        org_id: row.org_id.0,
        table_name: row.table_name,
        key: row.key,
        value_json: row.value_json,
        updated_at_micros: row.updated_at.0,
    }
}

#[derive(Debug, Deserialize)]
struct UpsertReq {
    value_json: Value,
}

#[derive(Debug, Deserialize)]
struct CreateTableReq {
    table_name: String,
    #[serde(default)]
    description: String,
    key_field: String,
    #[serde(default)]
    value_fields: Vec<ExtendValueField>,
}

#[derive(Debug, Serialize)]
struct TableUsageResp {
    kind: &'static str,
    id: String,
    name: String,
}

#[derive(Debug, Serialize)]
struct TableResp {
    #[serde(flatten)]
    summary: ExtendTableSummary,
    usage_locations: Vec<TableUsageResp>,
}

#[permission("functions.read")]
async fn list_tables(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<TableResp>>> {
    let tables = state.storage.extend_kv.list_tables(&ctx.org_id).await?;
    let pipelines = state.storage.scheduled_pipelines.list(&ctx.org_id).await?;
    let saved_views = state.platform.saved_view.list(&ctx.org_id, false).await?;
    Ok(Json(
        tables
            .into_iter()
            .map(|summary| {
                let mut usage_locations = pipeline_usages(&pipelines, &summary.table_name);
                usage_locations.extend(saved_view_usages(&saved_views, &summary.table_name));
                TableResp {
                    summary,
                    usage_locations,
                }
            })
            .collect(),
    ))
}

#[permission("functions.create")]
async fn create_table(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateTableReq>,
) -> Result<Json<TableResp>> {
    let table_name = req.table_name.trim().to_string();
    let key_field = req.key_field.trim().to_string();
    validate_schema_name(&table_name, "table_name")?;
    validate_schema_name(&key_field, "key_field")?;
    if req.description.len() > 2_000 {
        return Err(Error::invalid("description cannot exceed 2000 characters"));
    }
    if req.value_fields.len() > 100 {
        return Err(Error::invalid("value_fields cannot exceed 100 fields"));
    }
    let mut seen = HashSet::new();
    let value_fields = req
        .value_fields
        .into_iter()
        .map(|field| normalize_field(field, &mut seen))
        .collect::<Result<Vec<_>>>()?;
    if state
        .storage
        .extend_kv
        .list_tables(&ctx.org_id)
        .await?
        .iter()
        .any(|table| table.table_name == table_name)
    {
        return Err(Error::conflict(format!(
            "extend table {table_name} already exists"
        )));
    }
    let now = TimestampMicros::now();
    let table = state
        .storage
        .extend_kv
        .create_table(ExtendTableDefinition {
            org_id: ctx.org_id.clone(),
            table_name: table_name.clone(),
            description: req.description.trim().to_string(),
            key_field,
            value_fields,
            created_at: now,
            updated_at: now,
        })
        .await?;
    Ok(Json(TableResp {
        summary: ExtendTableSummary {
            table_name: table.table_name,
            description: table.description,
            key_field: table.key_field,
            value_fields: table.value_fields,
            row_count: 0,
            updated_at: table.updated_at,
        },
        usage_locations: Vec::new(),
    }))
}

#[permission("functions.read")]
async fn list_rows(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(table): Path<String>,
) -> Result<Json<Vec<RowResp>>> {
    if table.trim().is_empty() {
        return Err(Error::invalid("table cannot be empty"));
    }
    Ok(Json(
        state
            .storage
            .extend_kv
            .list_table(&ctx.org_id, &table)
            .await?
            .into_iter()
            .map(row_resp)
            .collect(),
    ))
}

#[permission("functions.delete")]
async fn delete_table(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(table): Path<String>,
) -> Result<Json<Value>> {
    validate_key(&table, "table")?;
    state
        .storage
        .extend_kv
        .delete_table(&ctx.org_id, &table)
        .await?;
    state.storage.extend_table.drop_table(&ctx.org_id, &table);
    Ok(Json(serde_json::json!({ "deleted": true })))
}

#[permission("functions.edit")]
async fn upsert_row(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((table, key)): Path<(String, String)>,
    Json(req): Json<UpsertReq>,
) -> Result<Json<Value>> {
    validate_key(&table, "table")?;
    validate_key(&key, "key")?;
    let now = TimestampMicros::now();
    let row = ExtendRow {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        table_name: table.clone(),
        key,
        value_json: req.value_json,
        updated_at: now,
    };
    state.storage.extend_kv.upsert(row).await?;
    refresh_table(&state, &ctx.org_id, &table).await?;
    Ok(Json(serde_json::json!({ "updated": true })))
}

#[permission("functions.edit")]
async fn delete_row(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((table, key)): Path<(String, String)>,
) -> Result<Json<Value>> {
    validate_key(&table, "table")?;
    validate_key(&key, "key")?;
    state
        .storage
        .extend_kv
        .delete(&ctx.org_id, &table, &key)
        .await?;
    refresh_table(&state, &ctx.org_id, &table).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

fn validate_key(value: &str, label: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::invalid(format!("{label} cannot be empty")));
    }
    Ok(())
}

fn validate_schema_name(value: &str, label: &str) -> Result<()> {
    validate_key(value, label)?;
    if value.len() > 255 {
        return Err(Error::invalid(format!(
            "{label} cannot exceed 255 characters"
        )));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
    {
        return Err(Error::invalid(format!(
            "{label} can only contain letters, numbers, '.', '-' and '_'"
        )));
    }
    Ok(())
}

fn normalize_field(
    mut field: ExtendValueField,
    seen: &mut HashSet<String>,
) -> Result<ExtendValueField> {
    field.name = field.name.trim().to_string();
    field.field_type = field.field_type.trim().to_ascii_lowercase();
    field.description = field.description.trim().to_string();
    validate_schema_name(&field.name, "field name")?;
    if !seen.insert(field.name.clone()) {
        return Err(Error::invalid(format!(
            "duplicate field name: {}",
            field.name
        )));
    }
    if !matches!(
        field.field_type.as_str(),
        "string" | "number" | "boolean" | "object"
    ) {
        return Err(Error::invalid(format!(
            "unsupported field type: {}",
            field.field_type
        )));
    }
    if field.description.len() > 500 {
        return Err(Error::invalid(format!(
            "description for field {} cannot exceed 500 characters",
            field.name
        )));
    }
    Ok(field)
}

fn pipeline_usages(pipelines: &[ScheduledPipeline], table_name: &str) -> Vec<TableUsageResp> {
    pipelines
        .iter()
        .filter(|pipeline| value_references_table(&pipeline.function_steps, table_name))
        .map(|pipeline| TableUsageResp {
            kind: "pipeline",
            id: pipeline.id.0.clone(),
            name: pipeline.name.clone(),
        })
        .collect()
}

fn saved_view_usages(saved_views: &[SavedView], table_name: &str) -> Vec<TableUsageResp> {
    saved_views
        .iter()
        .filter(|view| {
            view.stream.as_deref() == Some(table_name)
                || text_references_identifier(&view.statement, table_name)
        })
        .map(|view| TableUsageResp {
            kind: "saved_view",
            id: view.id.0.clone(),
            name: view.name.clone(),
        })
        .collect()
}

fn value_references_table(value: &Value, table_name: &str) -> bool {
    match value {
        Value::String(text) => text == table_name,
        Value::Array(items) => items
            .iter()
            .any(|item| value_references_table(item, table_name)),
        Value::Object(fields) => fields
            .values()
            .any(|field| value_references_table(field, table_name)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

fn text_references_identifier(text: &str, identifier: &str) -> bool {
    text.match_indices(identifier).any(|(start, _)| {
        let before = text[..start].chars().next_back();
        let end = start + identifier.len();
        let after = text[end..].chars().next();
        before.is_none_or(|character| !is_identifier_character(character))
            && after.is_none_or(|character| !is_identifier_character(character))
    })
}

fn is_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
}

async fn refresh_table(state: &AppState, org_id: &Id, table: &str) -> Result<()> {
    let rows = state.storage.extend_kv.list_table(org_id, table).await?;
    if rows.is_empty() {
        state.storage.extend_table.drop_table(org_id, table);
        return Ok(());
    }
    let map: HashMap<String, Value> = rows
        .into_iter()
        .map(|row| (row.key, row.value_json))
        .collect();
    state.storage.extend_table.replace_table(org_id, table, map);
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{text_references_identifier, value_references_table};

    #[test]
    fn detects_exact_table_reference_in_nested_pipeline_steps() {
        let steps = json!([
            {
                "kind": "lookup",
                "config": { "table": "customers" }
            }
        ]);
        assert!(value_references_table(&steps, "customers"));
        assert!(!value_references_table(&steps, "customer"));
    }

    #[test]
    fn saved_query_reference_requires_identifier_boundaries() {
        assert!(text_references_identifier(
            "SELECT * FROM customers WHERE tier = 'pro'",
            "customers"
        ));
        assert!(!text_references_identifier(
            "SELECT * FROM archived_customers",
            "customers"
        ));
    }
}
