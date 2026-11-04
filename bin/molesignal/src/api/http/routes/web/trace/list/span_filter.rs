// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Join raw Span matches back to the narrow one-row-per-Trace summary dataset.

use std::sync::Arc;

use arrow::{
    array::{Array, Float64Array, Int64Array, RecordBatch, StringArray, UInt64Array},
    datatypes::DataType,
};
use datafusion::{
    common::TableReference, execution::object_store::ObjectStoreUrl, prelude::SessionContext,
};

use self::order::{cursor_predicate, order_by};
use super::{TraceListContext, TraceListRow, filter::TraceFilter};
use crate::{
    api::{AppState, http::routes::web::trace::HTTP_STATUS_FIELDS},
    domain::{
        storage::{DatasetTypeId, primary_dataset_type, type_id::builtin},
        stream::{FieldType, Schema, StreamDefinition},
    },
    infra::{
        query::{escape_sql_ident, parquet_table::PrunedParquetTable},
        storage::{arrow_schema, org_scoped_store::scoped_to_org},
    },
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
        trace::summary::{
            TRACE_SUMMARY_DURATION_NS_FIELD, TRACE_SUMMARY_ERROR_COUNT_FIELD,
            TRACE_SUMMARY_SPAN_COUNT_FIELD, TRACE_SUMMARY_START_NS_FIELD,
        },
    },
};

pub(super) mod count;
mod order;
mod plan;

const RAW_TABLE: &str = "__molesignal_trace_spans";
const SUMMARY_TABLE: &str = "__molesignal_trace_summaries";
const SUMMARY_RANK_FIELD: &str = "__molesignal_summary_rank";

/// The spans and the summaries of a stream in a time range, registered as the
/// two tables a Span-filter statement reads.
struct SpanFilterTables {
    session: SessionContext,
    raw_files: usize,
    summary_files: usize,
    /// Rows the two datasets hold in the range, before any filter.
    candidate_rows: u64,
}

pub(super) async fn run(
    state: &AppState,
    definition: &StreamDefinition,
    context: &TraceListContext,
    fetch_limit: usize,
) -> Result<Vec<TraceListRow>> {
    let range = TimeRange::new(TimestampMicros(context.from), TimestampMicros(context.to));

    let Some(tables) = register_tables(state, definition, range).await? else {
        return Ok(Vec::new());
    };

    let sql = build_sql(context, &definition.schema, fetch_limit)?;

    let batches = tables
        .session
        .sql(&sql)
        .await
        .map_err(|error| Error::internal(format!("plan filtered trace summaries: {error}")))?
        .collect()
        .await
        .map_err(|error| Error::internal(format!("scan filtered trace summaries: {error}")))?;

    let rows = rows_from_batches(&batches);
    tracing::debug!(
        raw_files = tables.raw_files,
        summary_files = tables.summary_files,
        returned = rows.len(),
        "trace Span filters joined to summary projection"
    );
    Ok(rows)
}

/// `None` when either dataset holds nothing in the range: no trace can match.
async fn register_tables(
    state: &AppState,
    definition: &StreamDefinition,
    range: TimeRange,
) -> Result<Option<SpanFilterTables>> {
    let raw_type = primary_dataset_type(definition.stream_type)?;
    let summary_type = DatasetTypeId::builtin(builtin::DATASET_TRACE_SUMMARY);

    let snapshot = state
        .storage
        .catalog_query
        .snapshot_stream(definition, &[raw_type.clone(), summary_type.clone()], range)
        .await?;

    let (raw_files, raw_buffers) = snapshot
        .dataset(&raw_type)
        .map(|dataset| (dataset.files.clone(), dataset.buffered_batches.clone()))
        .unwrap_or_default();

    if raw_files.is_empty() && raw_buffers.is_empty() {
        return Ok(None);
    }

    let (summary_files, summary_buffers) = snapshot
        .dataset(&summary_type)
        .map(|dataset| (dataset.files.clone(), dataset.buffered_batches.clone()))
        .unwrap_or_default();

    if summary_files.is_empty() && summary_buffers.is_empty() {
        return Ok(None);
    }

    let candidate_rows = raw_files
        .iter()
        .chain(&summary_files)
        .map(|file| file.rows)
        .chain(
            raw_buffers
                .iter()
                .chain(&summary_buffers)
                .map(|batch| batch.num_rows() as u64),
        )
        .fold(0_u64, u64::saturating_add);

    let ctx = SessionContext::new();
    let object_store_url = ObjectStoreUrl::parse("molesignal://trace-list")
        .map_err(|error| Error::internal(format!("trace-list object store URL: {error}")))?;
    ctx.runtime_env().register_object_store(
        object_store_url.as_ref(),
        scoped_to_org(state.storage.read_store.clone(), &definition.org_id),
    );

    let raw_table = PrunedParquetTable::new(
        arrow_schema::to_arrow(&definition.schema),
        &raw_files,
        object_store_url.clone(),
        range,
        Some(raw_type),
    )
    .with_buffered_batches(raw_buffers);

    ctx.register_table(TableReference::bare(RAW_TABLE), Arc::new(raw_table))
        .map_err(|error| Error::internal(format!("register trace Span table: {error}")))?;

    let summary_definition =
        crate::infra::intake::physical_schema::project(definition, &summary_type);

    let summary_table = PrunedParquetTable::new(
        arrow_schema::to_arrow(&summary_definition.schema),
        &summary_files,
        object_store_url,
        range,
        Some(summary_type),
    )
    .with_buffered_batches(summary_buffers);

    ctx.register_table(TableReference::bare(SUMMARY_TABLE), Arc::new(summary_table))
        .map_err(|error| Error::internal(format!("register trace summary table: {error}")))?;

    Ok(Some(SpanFilterTables {
        session: ctx,
        raw_files: raw_files.len(),
        summary_files: summary_files.len(),
        candidate_rows,
    }))
}

fn build_sql(context: &TraceListContext, schema: &Schema, fetch_limit: usize) -> Result<String> {
    let from_ns = context
        .from
        .checked_mul(1_000)
        .ok_or_else(|| Error::invalid("trace time_from is out of range"))?;

    let to_ns = context
        .to
        .checked_mul(1_000)
        .ok_or_else(|| Error::invalid("trace time_to is out of range"))?;

    let plan = plan::plan(context)?;
    let start = qualified("s", TRACE_SUMMARY_START_NS_FIELD);
    let duration = qualified("s", TRACE_SUMMARY_DURATION_NS_FIELD);
    let span_count = qualified("s", TRACE_SUMMARY_SPAN_COUNT_FIELD);
    let error_count = qualified("s", TRACE_SUMMARY_ERROR_COUNT_FIELD);
    let trace_id = qualified("s", "trace_id");
    let mut predicates = vec![format!("{} = 1", qualified("s", SUMMARY_RANK_FIELD))];
    predicates.extend(plan.predicates);

    if let Some(boundary) = context.boundary.as_ref() {
        predicates.push(cursor_predicate(context, boundary.direction));
    }

    let order = order_by(context);

    // The HTTP status of the root, where the stream keeps one, as the summary copied it.
    let http_status = HTTP_STATUS_FIELDS
        .iter()
        .enumerate()
        .filter(|(_, name)| schema.fields.iter().any(|field| field.name == **name))
        .map(|(index, name)| format!(",\n{} AS http_status_{index}", qualified("s", name)))
        .collect::<String>();

    Ok(format!(
        "WITH ranked_summaries AS (\n\
           SELECT summary.*,\n\
                  ROW_NUMBER() OVER (\n\
                    PARTITION BY \"trace_id\"\n\
                    ORDER BY \"{start_field}\" DESC, \"trace_id\" DESC\n\
                  ) AS \"{rank_field}\"\n\
           FROM \"{summary_table}\" AS summary\n\
           WHERE \"{start_field}\" >= {from_ns}\n\
             AND \"{start_field}\" < {to_ns}\n\
         ),\n\
         matching_trace_ids AS (\n\
           {}\n\
         )\n\
         SELECT {trace_id} AS trace_id,\n\
                {} AS root_span_id,\n\
                {} AS service,\n\
                {} AS operation,\n\
                {} AS status,\n\
                {start} AS start_ns,\n\
                {duration} AS duration_ns,\n\
                {span_count} AS span_count,\n\
                {error_count} AS error_count{http_status}\n\
         FROM ranked_summaries AS s\n\
         INNER JOIN matching_trace_ids AS m\n\
           ON {trace_id} = {}\n\
         WHERE {}\n\
         ORDER BY {order}\n\
         LIMIT {fetch_limit}",
        plan.matching,
        qualified("s", "span_id"),
        qualified("s", "service.name"),
        qualified("s", "name"),
        qualified("s", "status_code"),
        qualified("m", "trace_id"),
        predicates.join(" AND "),
        start_field = TRACE_SUMMARY_START_NS_FIELD,
        rank_field = SUMMARY_RANK_FIELD,
        summary_table = SUMMARY_TABLE,
    ))
}

/// `filter`'s value compared by `op` with `field` of the row `alias`.
fn filter_predicate(alias: &str, filter: &TraceFilter, field: &str, op: &str) -> Result<String> {
    let column = qualified(alias, field);

    let expression = match filter.data_type {
        FieldType::Timestamp => format!("CAST({column} AS BIGINT)"),
        _ => column,
    };

    if op == "contains" {
        return Ok(format!(
            "CAST({expression} AS VARCHAR) LIKE {}",
            sql_literal(&format!("%{}%", filter.value))
        ));
    }

    if op == "in" {
        let listed = filter
            .values
            .iter()
            .map(|value| typed_literal(filter, field, value))
            .collect::<Result<Vec<_>>>()?;

        return Ok(format!("{expression} IN ({})", listed.join(", ")));
    }

    Ok(format!(
        "{expression} {op} {}",
        typed_literal(filter, field, &filter.value)?
    ))
}

/// A value of `filter`'s field, already checked against its type, as SQL.
fn typed_literal(filter: &TraceFilter, field: &str, value: &str) -> Result<String> {
    match filter.data_type {
        FieldType::Utf8 => Ok(sql_literal(value)),
        FieldType::Int64 | FieldType::Float64 | FieldType::Timestamp => Ok(value.to_string()),
        FieldType::Bool => Ok(value.to_ascii_uppercase()),
        FieldType::Json => Err(Error::invalid(format!(
            "trace field `{field}` requires a JSON path query in SQL mode"
        ))),
    }
}

fn rows_from_batches(batches: &[RecordBatch]) -> Vec<TraceListRow> {
    let mut rows = Vec::new();

    for batch in batches {
        for row in 0..batch.num_rows() {
            let Some(trace_id) = string_at(batch, "trace_id", row) else {
                continue;
            };

            let duration_ns = integer_at(batch, "duration_ns", row)
                .unwrap_or_default()
                .max(0);

            let text = |name| string_at(batch, name, row).unwrap_or_default().to_string();

            rows.push(TraceListRow {
                item: super::TraceListItem {
                    trace_id: trace_id.to_string(),
                    service: text("service"),
                    operation: text("operation"),
                    start_ns: integer_at(batch, "start_ns", row).unwrap_or_default(),
                    duration_ms: duration_ns as f64 / 1_000_000.0,
                    span_count: integer_at(batch, "span_count", row).unwrap_or_default(),
                    error_count: integer_at(batch, "error_count", row).unwrap_or_default(),
                    status: text("status"),
                    http_status_code: (0..HTTP_STATUS_FIELDS.len())
                        .find_map(|index| integer_at(batch, &format!("http_status_{index}"), row))
                        .filter(|status| (100..600).contains(status)),
                    ..Default::default()
                },
                duration_ns,
                root_span_id: text("root_span_id"),
            });
        }
    }

    rows
}

fn qualified(alias: &str, field: &str) -> String {
    format!("{alias}.\"{}\"", escape_sql_ident(field))
}

fn sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn string_at<'a>(batch: &'a RecordBatch, name: &str, row: usize) -> Option<&'a str> {
    let column = batch.column_by_name(name)?;
    let values = column.as_any().downcast_ref::<StringArray>()?;
    values.is_valid(row).then(|| values.value(row))
}

fn integer_at(batch: &RecordBatch, name: &str, row: usize) -> Option<i64> {
    let column = batch.column_by_name(name)?;

    if column.is_null(row) {
        return None;
    }

    match column.data_type() {
        DataType::Int64 => Some(column.as_any().downcast_ref::<Int64Array>()?.value(row)),
        DataType::UInt64 => {
            i64::try_from(column.as_any().downcast_ref::<UInt64Array>()?.value(row)).ok()
        }
        DataType::Float64 => {
            Some(column.as_any().downcast_ref::<Float64Array>()?.value(row) as i64)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::{
        array::{Int64Array, RecordBatch, StringArray},
        datatypes::{DataType, Field, Schema as ArrowSchema},
    };
    use datafusion::{datasource::MemTable, prelude::SessionContext};
    use serde_json::{Value, json};

    use super::*;
    use crate::{
        api::http::routes::web::trace::list::TraceListSort, domain::stream::FieldDef,
        shared::trace::normalization::canonical_trace_schema,
    };

    fn schema() -> Schema {
        let mut schema = canonical_trace_schema();

        schema.fields.push(FieldDef {
            name: "http.status_code".into(),
            data_type: FieldType::Int64,
            nullable: true,
            index_type: None,
            indexed: false,
            encrypted: false,
            exact: false,
        });

        schema
    }

    fn context(filters: Value) -> TraceListContext {
        let filters =
            super::super::filter::parse(Some(&filters.to_string()), &schema(), 32).unwrap();

        TraceListContext {
            from: 0,
            to: 1_000,
            sort: TraceListSort::Latest,
            page_size: 20,
            q: None,
            filters,
            condition: None,
            boundary: None,
        }
    }

    fn failing_and_long() -> Value {
        json!([
            {"field": "http.status_code", "op": ">=", "value": "500"},
            {"field": "span_count", "op": ">=", "value": "2"}
        ])
    }

    /// Three traces, the latest first. The one Span of `trace-c` keeps neither
    /// a service nor a status code; in `trace-b`, `checkout` answers 200 and
    /// `payments` 503; `trace-a` is one Span of `web` that answers 200.
    fn session() -> SessionContext {
        let session = SessionContext::new();

        let raw_schema = Arc::new(ArrowSchema::new(vec![
            Field::new("trace_id", DataType::Utf8, false),
            Field::new("service.name", DataType::Utf8, true),
            Field::new("http.status_code", DataType::Int64, true),
        ]));

        let raw = RecordBatch::try_new(
            raw_schema.clone(),
            vec![
                Arc::new(StringArray::from(vec![
                    "trace-a", "trace-b", "trace-b", "trace-c",
                ])),
                Arc::new(StringArray::from(vec![
                    Some("web"),
                    Some("checkout"),
                    Some("payments"),
                    None,
                ])),
                Arc::new(Int64Array::from(vec![
                    Some(200),
                    Some(200),
                    Some(503),
                    None,
                ])),
            ],
        )
        .unwrap();

        session
            .register_table(
                RAW_TABLE,
                Arc::new(MemTable::try_new(raw_schema, vec![vec![raw]]).unwrap()),
            )
            .unwrap();

        let summary_schema = Arc::new(ArrowSchema::new(vec![
            Field::new("trace_id", DataType::Utf8, false),
            Field::new("span_id", DataType::Utf8, true),
            Field::new("parent_span_id", DataType::Utf8, true),
            Field::new("service.name", DataType::Utf8, true),
            Field::new("name", DataType::Utf8, true),
            Field::new("status_code", DataType::Utf8, true),
            Field::new("http.status_code", DataType::Int64, true),
            Field::new(TRACE_SUMMARY_START_NS_FIELD, DataType::Int64, false),
            Field::new(TRACE_SUMMARY_DURATION_NS_FIELD, DataType::Int64, false),
            Field::new(TRACE_SUMMARY_SPAN_COUNT_FIELD, DataType::Int64, false),
            Field::new(TRACE_SUMMARY_ERROR_COUNT_FIELD, DataType::Int64, false),
        ]));

        let summary = RecordBatch::try_new(
            summary_schema.clone(),
            vec![
                Arc::new(StringArray::from(vec!["trace-a", "trace-b", "trace-c"])),
                Arc::new(StringArray::from(vec!["a-root", "b-root", "c-root"])),
                Arc::new(StringArray::from(vec![None::<&str>, None, None])),
                Arc::new(StringArray::from(vec![Some("web"), Some("checkout"), None])),
                Arc::new(StringArray::from(vec!["GET /", "POST /checkout", "job"])),
                Arc::new(StringArray::from(vec!["OK", "ERROR", "OK"])),
                Arc::new(Int64Array::from(vec![Some(200), Some(200), None])),
                Arc::new(Int64Array::from(vec![100, 200, 300])),
                Arc::new(Int64Array::from(vec![10, 20, 30])),
                Arc::new(Int64Array::from(vec![1, 2, 1])),
                Arc::new(Int64Array::from(vec![0, 1, 0])),
            ],
        )
        .unwrap();

        session
            .register_table(
                SUMMARY_TABLE,
                Arc::new(MemTable::try_new(summary_schema, vec![vec![summary]]).unwrap()),
            )
            .unwrap();

        session
    }

    async fn matched(filters: Value) -> Vec<TraceListRow> {
        let batches = session()
            .sql(&build_sql(&context(filters), &schema(), 21).unwrap())
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();

        rows_from_batches(&batches)
    }

    async fn matched_ids(filters: Value) -> Vec<String> {
        matched(filters)
            .await
            .into_iter()
            .map(|row| row.item.trace_id)
            .collect()
    }

    #[tokio::test]
    async fn any_span_match_returns_values_from_the_summary_row() {
        let rows = matched(failing_and_long()).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].item.trace_id, "trace-b");
        assert_eq!(rows[0].item.service, "checkout");
        assert_eq!(rows[0].item.span_count, 2);
        assert_eq!(rows[0].item.status, "ERROR");
        assert_eq!(rows[0].item.http_status_code, Some(200));
        assert_eq!(rows[0].root_span_id, "b-root");
    }

    #[tokio::test]
    async fn in_keeps_the_traces_with_a_span_of_any_of_its_values() {
        let services =
            json!([{"field": "service.name", "op": "in", "values": ["payments", "web"]}]);
        assert_eq!(matched_ids(services).await, ["trace-b", "trace-a"]);

        let counts = json!([{"field": "span_count", "op": "in", "values": ["2", "9"]}]);
        assert_eq!(
            matched_ids(json!([
                {"field": "service.name", "op": "in", "values": ["checkout"]},
                counts[0]
            ]))
            .await,
            ["trace-b"]
        );
    }

    #[tokio::test]
    async fn leaving_a_value_out_keeps_the_traces_none_of_whose_spans_has_it() {
        let payments = |op: &str| json!([{"field": "service.name", "op": op, "value": "payments"}]);

        // A Span without a service is not one of `payments`: `trace-c` stays.
        assert_eq!(matched_ids(payments("!=")).await, ["trace-c", "trace-a"]);
        assert_eq!(matched_ids(payments("=")).await, ["trace-b"]);

        // `trace-b` has a Span that answered otherwise, and one that answered 200.
        let ok = json!([{"field": "http.status_code", "op": "!=", "value": "200"}]);
        assert_eq!(matched_ids(ok).await, ["trace-c"]);
    }

    #[test]
    fn generated_sql_uses_having_for_any_span_and_summary_predicates_for_counts() {
        let sql = build_sql(&context(failing_and_long()), &schema(), 21).unwrap();
        assert!(sql.contains("MAX(CASE WHEN r.\"http.status_code\" >= 500 THEN 1 ELSE 0 END) = 1"));
        assert!(sql.contains("s.\"molesignal.trace.span_count\" >= 2"));
        assert!(sql.contains("LIMIT 21"));
    }

    #[test]
    fn leaving_a_value_out_asks_that_no_span_has_it() {
        let excluded = json!([{"field": "service.name", "op": "ne", "value": "payments"}]);
        let sql = build_sql(&context(excluded), &schema(), 21).unwrap();

        assert!(
            sql.contains("MAX(CASE WHEN r.\"service.name\" = 'payments' THEN 1 ELSE 0 END) = 0")
        );
    }

    fn service(op: &str, value: &str) -> Value {
        json!({"field": "service.name", "op": op, "value": value})
    }

    fn context_where(filters: Value, condition: Value) -> TraceListContext {
        let mut context = context(filters);

        context.condition =
            super::super::filter::condition::parse(Some(&condition.to_string()), &schema(), 32)
                .unwrap();

        context
    }

    async fn matched_where(filters: Value, condition: Value) -> Vec<String> {
        let batches = session()
            .sql(&build_sql(&context_where(filters, condition), &schema(), 21).unwrap())
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();

        rows_from_batches(&batches)
            .into_iter()
            .map(|row| row.item.trace_id)
            .collect()
    }

    #[tokio::test]
    async fn either_span_condition_keeps_a_trace() {
        let either = json!({"or": [service("=", "payments"), service("=", "web")]});
        assert_eq!(
            matched_where(json!([]), either).await,
            ["trace-b", "trace-a"]
        );

        // A Span without the fields answers neither: `trace-c` has no such Span.
        let checkout_or_ok = json!({"or": [
            service("=", "checkout"),
            {"field": "http.status_code", "op": "=", "value": "200"}
        ]});
        assert_eq!(
            matched_where(json!([]), checkout_or_ok).await,
            ["trace-b", "trace-a"]
        );

        // No Span of `trace-c` has `payments`, nor does any of `trace-a`; `trace-b` failed with a 503.
        let left_out_or_failed = json!({"or": [
            service("!=", "payments"),
            {"field": "http.status_code", "op": ">=", "value": "500"}
        ]});
        assert_eq!(
            matched_where(json!([]), left_out_or_failed).await,
            ["trace-c", "trace-b", "trace-a"]
        );
    }

    #[tokio::test]
    async fn a_span_and_a_trace_condition_are_read_together_for_each_trace() {
        let web_or_long = json!({"or": [
            service("=", "web"),
            {"field": "span_count", "op": ">=", "value": "2"}
        ]});
        assert_eq!(
            matched_where(json!([]), web_or_long).await,
            ["trace-b", "trace-a"]
        );

        // Free text is looked for in the summary: `job` names the root of `trace-c`.
        let job_or_web = json!({"or": [{"text": "JOB"}, service("=", "web")]});
        assert_eq!(
            matched_where(json!([]), job_or_web).await,
            ["trace-c", "trace-a"]
        );

        // The list's filters hold beside the tree.
        let failed = json!([{"field": "error_count", "op": ">=", "value": "1"}]);
        let any_service = json!({"or": [service("=", "web"), service("=", "payments")]});
        assert_eq!(matched_where(failed, any_service).await, ["trace-b"]);
    }

    #[tokio::test]
    async fn the_tree_says_how_conditions_group() {
        // `web OR checkout AND span_count >= 5`: AND first.
        let and_first = json!({"or": [
            service("=", "web"),
            {"and": [service("=", "checkout"), {"field": "span_count", "op": ">=", "value": "5"}]}
        ]});
        assert_eq!(matched_where(json!([]), and_first).await, ["trace-a"]);

        // `(web OR checkout) AND span_count >= 2`.
        let bracketed = json!({"and": [
            {"or": [service("=", "web"), service("=", "checkout")]},
            {"field": "span_count", "op": ">=", "value": "2"}
        ]});
        assert_eq!(matched_where(json!([]), bracketed).await, ["trace-b"]);
    }

    #[test]
    fn a_part_of_span_conditions_alone_stays_in_having() {
        let either = json!({"or": [service("=", "payments"), service("=", "web")]});
        let sql = build_sql(&context_where(json!([]), either), &schema(), 21).unwrap();

        assert!(sql.contains(
            "HAVING (MAX(CASE WHEN r.\"service.name\" = 'payments' THEN 1 ELSE 0 END) = 1 OR \
             MAX(CASE WHEN r.\"service.name\" = 'web' THEN 1 ELSE 0 END) = 1)"
        ));
        assert!(!sql.contains("__molesignal_span_"));

        let mixed =
            json!({"or": [service("!=", "web"), {"field": "span_count", "op": ">", "value": "1"}]});
        let sql = build_sql(&context_where(json!([]), mixed), &schema(), 21).unwrap();

        assert!(sql.contains(
            "MAX(CASE WHEN r.\"service.name\" = 'web' THEN 1 ELSE 0 END) AS \"__molesignal_span_0\""
        ));
        assert!(
            sql.contains(
                "(m.\"__molesignal_span_0\" = 0 OR s.\"molesignal.trace.span_count\" > 1)"
            )
        );
        assert!(!sql.contains("HAVING"));
    }
}
