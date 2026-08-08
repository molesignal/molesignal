// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Structured log search with signed, bidirectional keyset pagination.

use axum::{Extension, Json, Router, extract::State, routing::post};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use self::cursor::{LogCursorBoundary, LogCursorPayload};
use crate::{
    api::{
        AppState,
        http::pagination::cursor::{CursorDirection, CursorPage, trim_cursor_page},
    },
    app::iam::IamContext,
    domain::{
        iam::permission,
        ingestion::EVENT_ID_FIELD,
        query::{QueryLanguage, QueryRequest, StreamHint},
        stream::StreamType,
    },
    infra::query::escape_sql_ident,
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

mod cursor;
mod filter;

const DEFAULT_WINDOW_MICROS: i64 = 24 * 60 * 60 * 1_000_000;
const DEFAULT_PAGE_SIZE: usize = 20;
pub(super) const MAX_PAGE_SIZE: usize = 100;
const MAX_FILTERS: usize = 32;
const MAX_FREE_TEXT: usize = 8;

pub fn routes() -> Router<AppState> {
    Router::new().route("/logs", post(list))
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(super) struct LogFilter {
    pub(super) field: String,
    pub(super) op: String,
    pub(super) value: String,
    #[serde(default)]
    pub(super) quoted: bool,
}

#[derive(Debug, Default, Deserialize)]
struct LogListRequest {
    stream: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    #[serde(default)]
    filters: Vec<LogFilter>,
    #[serde(default)]
    free_text: Vec<String>,
    limit: Option<usize>,
    cursor: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct LogListContext {
    stream: String,
    from: i64,
    to: i64,
    page_size: usize,
    filters: Vec<LogFilter>,
    free_text: Vec<String>,
    boundary: Option<LogCursorBoundary>,
}

#[derive(Debug)]
pub(super) struct LogListRow {
    item: Map<String, Value>,
    timestamp_micros: i64,
    event_id: String,
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn list(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Json(request): Json<LogListRequest>,
) -> Result<Json<CursorPage<Map<String, Value>>>> {
    let context = resolve_context(&state, &iam, request)?;
    let stream = match state
        .telemetry
        .streams
        .get(&iam.org_id, &context.stream, StreamType::Logs)
        .await
    {
        Ok(stream) => stream,
        Err(Error::NotFound(_)) => return Ok(Json(CursorPage::empty())),
        Err(error) => return Err(error),
    };
    let has_event_id = stream
        .schema
        .fields
        .iter()
        .any(|field| field.name == EVENT_ID_FIELD);
    if !has_event_id {
        return Err(Error::invalid(
            "log stream has no stable event id for cursor pagination",
        ));
    }
    validate_filter_fields(&context, &stream.schema.fields)?;

    let fetch_limit = context.page_size.saturating_add(1);
    let sql = list_sql(&context, fetch_limit, &stream.schema.fields)?;
    let output = state
        .query
        .run(QueryRequest {
            org_id: iam.org_id.clone(),
            language: QueryLanguage::Sql,
            statement: sql,
            time_range: TimeRange::new(TimestampMicros(context.from), TimestampMicros(context.to)),
            stream: Some(StreamHint {
                name: context.stream.clone(),
                stream_type: StreamType::Logs,
            }),
            limit: Some(fetch_limit),
            federation_clusters: Vec::new(),
        })
        .await?;
    let direction = context.boundary.as_ref().map(|boundary| boundary.direction);
    let page = trim_cursor_page(rows_from_query(output), context.page_size, direction);

    let previous_cursor = if page.has_previous {
        page.items
            .first()
            .map(|row| {
                cursor::encode(
                    state.iam.service.as_ref(),
                    &iam.org_id,
                    &context,
                    CursorDirection::Before,
                    row,
                )
            })
            .transpose()?
    } else {
        None
    };
    let next_cursor = if page.has_next {
        page.items
            .last()
            .map(|row| {
                cursor::encode(
                    state.iam.service.as_ref(),
                    &iam.org_id,
                    &context,
                    CursorDirection::After,
                    row,
                )
            })
            .transpose()?
    } else {
        None
    };

    Ok(Json(CursorPage {
        items: page.items.into_iter().map(|row| row.item).collect(),
        has_more: next_cursor.is_some(),
        next_cursor,
        previous_cursor,
    }))
}

fn resolve_context(
    state: &AppState,
    iam: &IamContext,
    request: LogListRequest,
) -> Result<LogListContext> {
    if let Some(token) = request.cursor.as_deref() {
        let payload = cursor::decode(state.iam.service.as_ref(), &iam.org_id, token)?;
        validate_cursor_request(&request, &payload)?;
        return Ok(LogListContext {
            stream: payload.stream,
            from: payload.from,
            to: payload.to,
            page_size: payload.page_size,
            filters: payload.filters,
            free_text: payload.free_text,
            boundary: Some(LogCursorBoundary {
                direction: payload.direction,
                timestamp_micros: payload.timestamp_micros,
                event_id: payload.event_id,
            }),
        });
    }

    let stream = clean_required(request.stream, "log stream is required", 128)?;
    let now = TimestampMicros::now().0;
    let from = request
        .from
        .unwrap_or_else(|| now.saturating_sub(DEFAULT_WINDOW_MICROS));
    let to = request.to.unwrap_or(now);
    if to <= from {
        return Err(Error::invalid("log range end must be greater than start"));
    }
    let page_size = request
        .limit
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);
    let filters = normalize_filters(request.filters)?;
    let free_text = normalize_free_text(request.free_text)?;
    Ok(LogListContext {
        stream,
        from,
        to,
        page_size,
        filters,
        free_text,
        boundary: None,
    })
}

fn validate_cursor_request(request: &LogListRequest, payload: &LogCursorPayload) -> Result<()> {
    let filters = normalize_filters(request.filters.clone())?;
    let free_text = normalize_free_text(request.free_text.clone())?;
    let mismatch = request
        .stream
        .as_deref()
        .is_some_and(|value| value != payload.stream)
        || request.from.is_some_and(|value| value != payload.from)
        || request.to.is_some_and(|value| value != payload.to)
        || request
            .limit
            .is_some_and(|value| value.clamp(1, MAX_PAGE_SIZE) != payload.page_size)
        || (!filters.is_empty() && filters != payload.filters)
        || (!free_text.is_empty() && free_text != payload.free_text);
    if mismatch {
        return Err(Error::invalid("log cursor does not match the active query"));
    }
    Ok(())
}

fn normalize_filters(filters: Vec<LogFilter>) -> Result<Vec<LogFilter>> {
    filter::normalize(filters, MAX_FILTERS)
}

fn normalize_free_text(values: Vec<String>) -> Result<Vec<String>> {
    if values.len() > MAX_FREE_TEXT {
        return Err(Error::invalid("too many log free-text clauses"));
    }
    values
        .into_iter()
        .map(|value| clean_required(Some(value), "empty log search text", 512))
        .collect()
}

fn clean_required(value: Option<String>, error: &str, max: usize) -> Result<String> {
    let value = value.unwrap_or_default();
    let value = value.trim();
    if value.is_empty() || value.len() > max || value.contains('\0') {
        return Err(Error::invalid(error));
    }
    Ok(value.to_string())
}

fn validate_filter_fields(
    context: &LogListContext,
    fields: &[crate::domain::stream::FieldDef],
) -> Result<()> {
    filter::validate(&context.filters, fields)?;
    if !context.free_text.is_empty() && !fields.iter().any(|field| field.name == "message") {
        return Err(Error::invalid("log stream has no message field"));
    }
    Ok(())
}

fn list_sql(
    context: &LogListContext,
    fetch_limit: usize,
    fields: &[crate::domain::stream::FieldDef],
) -> Result<String> {
    let stream = escape_sql_ident(&context.stream);
    // `_timestamp` is stored as Arrow Timestamp(Microsecond, UTC). Cursor payloads
    // and HTTP bounds use integer microseconds, so comparisons must use the same
    // integer projection; DataFusion does not coerce Timestamp >= Int64.
    let timestamp_expr = "CAST(_timestamp AS BIGINT)";
    // 比较/游标仍用微秒整数，但 ORDER BY 直接使用物理列。
    // Parquet 文件已按 `(_timestamp, _event_id) DESC NULLS LAST` 写入，
    // 避免 CAST/COALESCE 包裹后 DataFusion 无法利用声明的文件内顺序。
    let timestamp_order = "_timestamp";
    let event_id_expr = format!("COALESCE(\"{EVENT_ID_FIELD}\", '')");
    let event_id_order = format!("\"{EVENT_ID_FIELD}\"");
    let mut where_clauses = vec![
        format!("{timestamp_expr} >= {}", context.from),
        format!("{timestamp_expr} < {}", context.to),
    ];
    for filter in &context.filters {
        where_clauses.push(filter::to_sql(filter, fields)?);
    }
    for value in &context.free_text {
        if crate::infra::query::tantivy_pruner::can_prune_match_term(value) {
            where_clauses.push(format!("MATCH(message, {})", sql_literal(value)));
        } else {
            where_clauses.push(format!(
                "\"message\" LIKE {}",
                sql_literal(&format!("%{value}%")),
            ));
        }
    }
    if let Some(boundary) = &context.boundary {
        where_clauses.push(cursor::seek_predicate(boundary, &event_id_expr));
    }
    let reverse = context
        .boundary
        .as_ref()
        .is_some_and(|boundary| boundary.direction == CursorDirection::Before);
    let order = if reverse { "ASC" } else { "DESC" };
    Ok(format!(
        "SELECT *, {timestamp_expr} AS __cursor_timestamp, \
                {event_id_expr} AS __cursor_event_id \
         FROM \"{stream}\" \
         WHERE {} \
         ORDER BY {timestamp_order} {order} NULLS LAST, \
                  {event_id_order} {order} NULLS LAST \
         LIMIT {fetch_limit}",
        where_clauses.join(" AND "),
    ))
}

fn rows_from_query(output: crate::domain::query::QueryResult) -> Vec<LogListRow> {
    let timestamp_index = output
        .columns
        .iter()
        .position(|column| column == "__cursor_timestamp");
    let event_id_index = output
        .columns
        .iter()
        .position(|column| column == "__cursor_event_id");
    output
        .rows
        .into_iter()
        .filter_map(|row| {
            let timestamp_micros = timestamp_index
                .and_then(|index| row.get(index))
                .and_then(value_i64)?;
            let event_id = event_id_index
                .and_then(|index| row.get(index))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let mut item = Map::new();
            for (index, column) in output.columns.iter().enumerate() {
                if column == "__cursor_timestamp"
                    || column == "__cursor_event_id"
                    || column == EVENT_ID_FIELD
                {
                    continue;
                }
                item.insert(
                    column.clone(),
                    row.get(index).cloned().unwrap_or(Value::Null),
                );
            }
            Some(LogListRow {
                item,
                timestamp_micros,
                event_id,
            })
        })
        .collect()
}

fn value_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| value.as_f64().map(|value| value as i64))
}

pub(super) fn sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::{
        array::{RecordBatch, StringArray, TimestampMicrosecondArray},
        datatypes::{DataType, Field, Schema, TimeUnit},
    };
    use datafusion::{datasource::MemTable, prelude::SessionContext};

    use super::*;

    fn context() -> LogListContext {
        LogListContext {
            stream: "app_logs".into(),
            from: 100,
            to: 200,
            page_size: 20,
            filters: vec![LogFilter {
                field: "service".into(),
                op: "contains".into(),
                value: "api'".into(),
                quoted: true,
            }],
            free_text: vec!["timeout'".into()],
            boundary: None,
        }
    }

    #[test]
    fn list_sql_applies_strict_window_sort_and_page_size_plus_one() {
        let fields = [crate::domain::stream::FieldDef {
            name: "service".into(),
            data_type: crate::domain::stream::FieldType::Utf8,
            nullable: true,
            indexed: false,
            encrypted: false,
            exact: false,
        }];
        let sql = list_sql(&context(), 21, &fields).expect("sql");
        assert!(sql.contains("CAST(_timestamp AS BIGINT) >= 100"));
        assert!(sql.contains("CAST(_timestamp AS BIGINT) < 200"));
        assert!(sql.contains("ORDER BY _timestamp DESC NULLS LAST, \"_event_id\" DESC NULLS LAST"));
        assert!(sql.contains("LIMIT 21"));
        assert!(sql.contains("%api''%"));
        assert!(sql.contains("%timeout''%"));
    }

    #[test]
    fn list_sql_uses_match_only_for_safely_prunable_terms() {
        let mut context = context();
        context.filters.clear();
        context.free_text = vec!["timeout".into(), "Timeout Error".into()];

        let sql = list_sql(&context, 21, &[]).expect("sql");
        assert!(sql.contains("MATCH(message, 'timeout')"));
        assert!(sql.contains("\"message\" LIKE '%Timeout Error%'"));
    }

    #[tokio::test]
    async fn list_sql_plans_against_arrow_timestamp_column() {
        let schema = Arc::new(Schema::new(vec![
            Field::new(
                "_timestamp",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new(EVENT_ID_FIELD, DataType::Utf8, false),
        ]));
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![
                Arc::new(TimestampMicrosecondArray::from(vec![150_i64]).with_timezone("UTC")),
                Arc::new(StringArray::from(vec!["event-1"])),
            ],
        )
        .expect("record batch");
        let table = MemTable::try_new(schema, vec![vec![batch]]).expect("mem table");
        let session = SessionContext::new();
        session
            .register_table("app_logs", Arc::new(table))
            .expect("register table");

        let mut context = context();
        context.filters.clear();
        context.free_text.clear();
        let sql = list_sql(&context, 21, &[]).expect("sql");
        let batches = session
            .sql(&sql)
            .await
            .expect("timestamp comparison should plan")
            .collect()
            .await
            .expect("timestamp comparison should execute");

        assert_eq!(batches.iter().map(RecordBatch::num_rows).sum::<usize>(), 1);
    }

    #[tokio::test]
    async fn list_sql_match_filter_excludes_non_matching_log_levels() {
        let schema = Arc::new(Schema::new(vec![
            Field::new(
                "_timestamp",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new(EVENT_ID_FIELD, DataType::Utf8, false),
            Field::new("level", DataType::Utf8, false),
        ]));
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![
                Arc::new(
                    TimestampMicrosecondArray::from(vec![150_i64, 160_i64]).with_timezone("UTC"),
                ),
                Arc::new(StringArray::from(vec!["event-1", "event-2"])),
                Arc::new(StringArray::from(vec!["INFO", "ERROR"])),
            ],
        )
        .expect("record batch");
        let table = MemTable::try_new(schema, vec![vec![batch]]).expect("mem table");
        let session = SessionContext::new();
        session
            .register_table("app_logs", Arc::new(table))
            .expect("register table");

        let mut context = context();
        context.filters = vec![LogFilter {
            field: "level".into(),
            op: "match".into(),
            value: "info".into(),
            quoted: true,
        }];
        context.free_text.clear();
        let fields = [crate::domain::stream::FieldDef {
            name: "level".into(),
            data_type: crate::domain::stream::FieldType::Utf8,
            nullable: false,
            indexed: false,
            encrypted: false,
            exact: false,
        }];
        let sql = list_sql(&context, 21, &fields).expect("sql");
        let batches = session
            .sql(&sql)
            .await
            .expect("MATCH filter should plan")
            .collect()
            .await
            .expect("MATCH filter should execute");

        assert_eq!(batches.iter().map(RecordBatch::num_rows).sum::<usize>(), 1);
    }
}
