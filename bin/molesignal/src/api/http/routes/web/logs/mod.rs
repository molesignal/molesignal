// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Structured log search with signed, bidirectional keyset pagination.

use axum::{Extension, Json, Router, extract::State, routing::get};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use self::{
    cursor::{LogCursorBoundary, LogCursorPayload},
    filter::condition::{self, LogCondition},
};
use crate::{
    api::{
        AppState,
        http::pagination::cursor::{CursorDirection, CursorPage, trim_cursor_page},
    },
    app::iam::IamContext,
    domain::{
        iam::permission,
        intake::EVENT_ID_FIELD,
        query::{QueryLanguage, QueryRequest, StreamHint},
        stream::StreamType,
    },
    infra::query::escape_sql_ident,
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

mod address;
mod cursor;
mod expression;
mod field_stats;
mod filter;
mod histogram;

const DEFAULT_WINDOW_MICROS: i64 = 24 * 60 * 60 * 1_000_000;
const DEFAULT_PAGE_SIZE: usize = 20;
/// The most rows one answer holds. The Logs explorer offers pages up to this size
/// (`EXPLORER_PAGE_SIZE_OPTIONS` in the web app), so the two change together.
pub(super) const MAX_PAGE_SIZE: usize = 5_000;
const MAX_FILTERS: usize = 32;
const MAX_FREE_TEXT: usize = 8;
/// `_timestamp` is stored as Arrow Timestamp(Microsecond, UTC). Cursor payloads
/// and HTTP bounds use integer microseconds, so comparisons must use the same
/// integer projection; DataFusion does not coerce Timestamp >= Int64.
const TIMESTAMP_MICROS: &str = "CAST(_timestamp AS BIGINT)";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/logs", get(address::list).post(list))
        .route(
            "/logs/histogram",
            get(address::histogram).post(histogram::histogram),
        )
        .route(
            "/logs/field-stats",
            get(address::field_stats).post(field_stats::field_stats),
        )
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
#[serde(rename_all = "camelCase")]
struct LogListRequest {
    stream: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    #[serde(default, deserialize_with = "address::json_list")]
    filters: Vec<LogFilter>,
    #[serde(default, deserialize_with = "address::json_list")]
    free_text: Vec<String>,
    /// The condition tree: alternatives the lists cannot say. It holds
    /// together with the filters and the words to find.
    #[serde(default, rename = "where", deserialize_with = "address::json_value")]
    condition: Option<Value>,
    /// The field query as text, read as the Logs page reads it; it stands
    /// for `filters`, `free_text` and `where`.
    #[serde(default)]
    expression: Option<String>,
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
    condition: Option<LogCondition>,
    boundary: Option<LogCursorBoundary>,
}

#[derive(Debug)]
pub(super) struct LogListRow {
    item: Map<String, Value>,
    timestamp_micros: i64,
    event_id: String,
}

#[permission(any("logs.read", "sys.telemetry.read"))]
async fn list(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Json(request): Json<LogListRequest>,
) -> Result<Json<CursorPage<Map<String, Value>>>> {
    let context = resolve_context(&state, &iam, request)?;

    let stream = match state
        .telemetry
        .streams
        .get(&iam.org_id, &context.stream, StreamType::LOGS)
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

    validate_filter_fields(
        &context.filters,
        &context.free_text,
        context.condition.as_ref(),
        &stream.schema.fields,
    )?;

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
                stream_type: StreamType::LOGS,
            }),
            limit: Some(fetch_limit),
            step: None,
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
            condition: payload.condition,
            boundary: Some(LogCursorBoundary {
                direction: payload.direction,
                timestamp_micros: payload.timestamp,
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

    let (filters, free_text, condition) = expression::read_match(
        request.expression,
        request.filters,
        request.free_text,
        request.condition,
    )?;

    Ok(LogListContext {
        stream,
        from,
        to,
        page_size,
        filters,
        free_text,
        condition,
        boundary: None,
    })
}

fn validate_cursor_request(request: &LogListRequest, payload: &LogCursorPayload) -> Result<()> {
    let (filters, free_text, condition) = expression::read_match(
        request.expression.clone(),
        request.filters.clone(),
        request.free_text.clone(),
        request.condition.clone(),
    )?;

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
        || (!free_text.is_empty() && free_text != payload.free_text)
        || (condition.is_some() && condition != payload.condition);

    if mismatch {
        return Err(Error::invalid("log cursor does not match the active query"));
    }

    Ok(())
}

fn normalize_filters(filters: Vec<LogFilter>) -> Result<Vec<LogFilter>> {
    filter::normalize(filters, MAX_FILTERS)
}

/// The condition tree of a request, with the room the lists leave it.
fn read_condition(
    value: Option<Value>,
    filters: &[LogFilter],
    free_text: &[String],
) -> Result<Option<LogCondition>> {
    condition::read(
        value,
        MAX_FILTERS.saturating_sub(filters.len()),
        MAX_FREE_TEXT.saturating_sub(free_text.len()),
    )
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
    filters: &[LogFilter],
    free_text: &[String],
    condition: Option<&LogCondition>,
    fields: &[crate::domain::stream::FieldDef],
) -> Result<()> {
    filter::validate(filters, fields)?;

    if let Some(condition) = condition {
        condition::validate(condition, fields)?;
    }

    let finds_text = !free_text.is_empty() || condition.is_some_and(LogCondition::has_text);

    if finds_text && !fields.iter().any(|field| field.name == "message") {
        return Err(Error::invalid("log stream has no message field"));
    }

    Ok(())
}

/// The conditions of a structured log query: inside the window `[from, to)`,
/// passing every filter, holding every free-text term and the condition tree.
/// The list and the histogram select with exactly these, so the histogram
/// counts the rows the list pages through.
///
/// The parts of the tree that all hold are clauses of their own beside the
/// lists', so a tree of filters joined by AND reads as the lists would.
fn match_clauses(
    (from, to): (i64, i64),
    filters: &[LogFilter],
    free_text: &[String],
    condition: Option<&LogCondition>,
    fields: &[crate::domain::stream::FieldDef],
) -> Result<Vec<String>> {
    let mut clauses = vec![
        format!("{TIMESTAMP_MICROS} >= {from}"),
        format!("{TIMESTAMP_MICROS} < {to}"),
    ];

    for filter in filters {
        clauses.push(filter::to_sql(filter, fields)?);
    }

    clauses.extend(free_text.iter().map(|value| text_sql(value)));

    for part in condition.map(LogCondition::conjuncts).unwrap_or_default() {
        clauses.push(condition::to_sql(part, fields)?);
    }

    Ok(clauses)
}

/// A word or phrase to find in the message. A plain lower-case word of at most
/// 40 bytes matches whatever its case in the message, as `MATCH` does;
/// anything else matches as written. Both narrow the files read by the
/// message's index.
fn text_sql(value: &str) -> String {
    let plain = !value.is_empty()
        && value.len() <= 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());

    if plain {
        format!("MATCH(message, {})", sql_literal(value))
    } else {
        format!("\"message\" LIKE {}", sql_literal(&format!("%{value}%")))
    }
}

fn list_sql(
    context: &LogListContext,
    fetch_limit: usize,
    fields: &[crate::domain::stream::FieldDef],
) -> Result<String> {
    let stream = escape_sql_ident(&context.stream);
    let timestamp_expr = TIMESTAMP_MICROS;
    // 比较/游标仍用微秒整数，但 ORDER BY 直接使用物理列。
    // Parquet 文件已按 `(_timestamp, _event_id) DESC NULLS LAST` 写入，
    // 避免 CAST/COALESCE 包裹后 DataFusion 无法利用声明的文件内顺序。
    let timestamp_order = "_timestamp";
    let event_id_expr = format!("COALESCE(\"{EVENT_ID_FIELD}\", '')");
    let event_id_order = format!("\"{EVENT_ID_FIELD}\"");

    let mut where_clauses = match_clauses(
        (context.from, context.to),
        &context.filters,
        &context.free_text,
        context.condition.as_ref(),
        fields,
    )?;

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
            condition: None,
            boundary: None,
        }
    }

    #[test]
    fn list_sql_applies_strict_window_sort_and_page_size_plus_one() {
        let fields = [crate::domain::stream::FieldDef {
            name: "service".into(),
            data_type: crate::domain::stream::FieldType::Utf8,
            nullable: true,
            index_type: None,
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
            index_type: None,
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

    fn utf8(name: &str) -> crate::domain::stream::FieldDef {
        crate::domain::stream::FieldDef {
            name: name.into(),
            data_type: crate::domain::stream::FieldType::Utf8,
            nullable: true,
            index_type: None,
            indexed: false,
            encrypted: false,
            exact: false,
        }
    }

    fn filter(field: &str, value: &str) -> LogFilter {
        LogFilter {
            field: field.into(),
            op: "=".into(),
            value: value.into(),
            quoted: true,
        }
    }

    fn tree(value: serde_json::Value) -> Option<LogCondition> {
        condition::read(Some(value), MAX_FILTERS, MAX_FREE_TEXT).expect("a condition")
    }

    /// Five logs inside the window; `e3` keeps no level.
    fn five_logs() -> SessionContext {
        let schema = Arc::new(Schema::new(vec![
            Field::new(
                "_timestamp",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new(EVENT_ID_FIELD, DataType::Utf8, false),
            Field::new("level", DataType::Utf8, true),
            Field::new("service", DataType::Utf8, true),
            Field::new("message", DataType::Utf8, true),
        ]));
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![
                Arc::new(
                    TimestampMicrosecondArray::from(vec![110_i64, 120, 130, 140, 150])
                        .with_timezone("UTC"),
                ),
                Arc::new(StringArray::from(vec!["e1", "e2", "e3", "e4", "e5"])),
                Arc::new(StringArray::from(vec![
                    Some("ERROR"),
                    Some("INFO"),
                    None,
                    Some("WARN"),
                    Some("INFO"),
                ])),
                Arc::new(StringArray::from(vec!["api", "web", "api", "web", "api"])),
                Arc::new(StringArray::from(vec![
                    "upstream timeout",
                    "order accepted",
                    "connection reset by peer",
                    "slow",
                    "ok",
                ])),
            ],
        )
        .expect("record batch");
        let session = SessionContext::new();
        session
            .register_table(
                "app_logs",
                Arc::new(MemTable::try_new(schema, vec![vec![batch]]).expect("mem table")),
            )
            .expect("register table");
        session
    }

    /// The logs the list returns for the filters and the tree, newest first,
    /// run the way the engine runs the statement: MATCH as ILIKE.
    async fn listed(filters: Vec<LogFilter>, condition: serde_json::Value) -> Vec<String> {
        let mut context = context();
        context.filters = filters;
        context.free_text.clear();
        context.condition = tree(condition);

        let fields = [utf8("level"), utf8("service"), utf8("message")];
        let sql = list_sql(&context, 21, &fields).expect("sql");
        let rewritten = crate::infra::query::tantivy_pruner::rewrite_match_calls(&sql);

        let batches = five_logs()
            .sql(&rewritten)
            .await
            .expect("the statement should plan")
            .collect()
            .await
            .expect("the statement should execute");
        let (columns, rows) = crate::infra::query::arrow_json::batches_to_json(&batches);
        let id = columns
            .iter()
            .position(|column| column == "__cursor_event_id")
            .expect("event id");

        rows.iter()
            .map(|row| row[id].as_str().unwrap_or_default().to_string())
            .collect()
    }

    fn level(value: &str) -> serde_json::Value {
        serde_json::json!({"field": "level", "op": "=", "value": value, "quoted": true})
    }

    #[tokio::test]
    async fn either_condition_keeps_a_log() {
        // A log without a level is found by the words of its message.
        let either = serde_json::json!({"or": [level("ERROR"), {"text": "connection reset"}]});
        assert_eq!(listed(Vec::new(), either).await, ["e3", "e1"]);

        // The list's filters hold beside the tree.
        let quiet = serde_json::json!({"or": [level("INFO"), level("WARN")]});
        assert_eq!(
            listed(vec![filter("service", "web")], quiet).await,
            ["e4", "e2"]
        );

        // A single word runs as MATCH, whatever its case.
        let word = serde_json::json!({"or": [level("WARN"), {"text": "timeout"}]});
        assert_eq!(listed(Vec::new(), word).await, ["e4", "e1"]);
    }

    #[tokio::test]
    async fn the_tree_says_how_conditions_group() {
        let web =
            serde_json::json!({"field": "service", "op": "=", "value": "web", "quoted": true});

        // `(INFO OR WARN) AND service = web`.
        let bracketed = serde_json::json!({"and": [{"or": [level("INFO"), level("WARN")]}, web]});
        assert_eq!(listed(Vec::new(), bracketed).await, ["e4", "e2"]);

        // `INFO OR WARN AND service = web`: AND first.
        let and_first = serde_json::json!({"or": [level("INFO"), {"and": [level("WARN"), web]}]});
        assert_eq!(listed(Vec::new(), and_first).await, ["e5", "e4", "e2"]);
    }

    #[tokio::test]
    async fn a_log_without_the_field_is_kept_by_the_other_part() {
        let not_error =
            serde_json::json!({"field": "level", "op": "!=", "value": "ERROR", "quoted": true});
        let api =
            serde_json::json!({"field": "service", "op": "=", "value": "api", "quoted": true});

        // Compared with nothing, `e3` is not other than ERROR ...
        assert_eq!(
            listed(Vec::new(), serde_json::json!({"and": [not_error.clone()]})).await,
            ["e5", "e4", "e2"]
        );

        // ... but it is of `api`.
        let either = serde_json::json!({"or": [not_error, api]});
        assert_eq!(
            listed(Vec::new(), either).await,
            ["e5", "e4", "e3", "e2", "e1"]
        );
    }

    #[test]
    fn a_tree_of_filters_joined_by_and_reads_as_the_lists() {
        let fields = [utf8("level"), utf8("service"), utf8("message")];
        let mut listed = context();
        listed.filters = vec![filter("level", "ERROR"), filter("service", "api")];
        listed.free_text = vec!["timeout".into()];

        let mut joined = context();
        joined.filters.clear();
        joined.free_text.clear();
        joined.condition = tree(serde_json::json!({"and": [
            level("ERROR"),
            {"field": "service", "op": "=", "value": "api", "quoted": true},
            {"text": "timeout"}
        ]}));

        assert_eq!(
            list_sql(&listed, 21, &fields).unwrap(),
            list_sql(&joined, 21, &fields).unwrap()
        );
    }

    /// The engine narrows the files a statement reads by what the rows it
    /// returns have to meet: a value, a word, a bound. One part of an OR does
    /// not narrow them on its own, since a file without it may still hold the
    /// other part; with every part ruled out the file is not read.
    #[test]
    fn a_part_of_an_or_narrows_the_files_read_only_with_the_other_parts() {
        let fields = [utf8("level"), utf8("service"), utf8("message")];
        let mut context = context();
        context.filters = vec![filter("service", "api")];
        context.free_text = vec!["upstream".into()];
        context.condition = tree(serde_json::json!({"or": [level("ERROR"), {"text": "timeout"}]}));

        let sql = list_sql(&context, 21, &fields).expect("sql");
        let rewritten = crate::infra::query::tantivy_pruner::rewrite_match_calls(&sql);

        assert!(
            rewritten.contains("message ILIKE '%timeout%'"),
            "{rewritten}"
        );

        let where_tree = query_language::WhereTree::parse(&rewritten);

        let ruled_out = |needles: &[&str]| {
            where_tree
                .atoms()
                .filter(|(_, expr)| {
                    let text = expr.to_string();
                    needles.iter().any(|needle| text.contains(needle))
                })
                .fold(0, |refuted, (atom, _)| {
                    refuted | query_language::atom_bit(atom)
                })
        };

        assert!(!where_tree.may_hold(ruled_out(&["'api'"])));
        assert!(!where_tree.may_hold(ruled_out(&["upstream"])));
        assert!(where_tree.may_hold(ruled_out(&["timeout"])));
        assert!(!where_tree.may_hold(ruled_out(&["timeout", "ERROR"])));
    }
}
