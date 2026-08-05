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

use super::{TraceListContext, TraceListRow, TraceListSort, filter::TraceFilter};
use crate::{
    api::{AppState, http::pagination::cursor::CursorDirection},
    domain::{
        storage::PhysicalDatasetKind,
        stream::{FieldType, StreamDefinition, StreamType},
    },
    infra::{
        query::{escape_sql_ident, parquet_table::PrunedParquetTable},
        storage::arrow_schema,
    },
    shared::{
        Error, Result,
        cursor::{CursorSortDirection, CursorValue, lexicographic_seek},
        ids::Id,
        time::{TimeRange, TimestampMicros},
        trace::summary::{
            TRACE_SUMMARY_DURATION_NS_FIELD, TRACE_SUMMARY_ERROR_COUNT_FIELD,
            TRACE_SUMMARY_SPAN_COUNT_FIELD, TRACE_SUMMARY_START_NS_FIELD,
        },
    },
};

const RAW_TABLE: &str = "__molesignal_trace_spans";
const SUMMARY_TABLE: &str = "__molesignal_trace_summaries";
const SUMMARY_RANK_FIELD: &str = "__molesignal_summary_rank";

pub(super) async fn run(
    state: &AppState,
    org_id: &Id,
    definition: &StreamDefinition,
    context: &TraceListContext,
    fetch_limit: usize,
) -> Result<Vec<TraceListRow>> {
    let range = TimeRange::new(TimestampMicros(context.from), TimestampMicros(context.to));
    let raw_files = state
        .storage
        .parquet_file_meta
        .find_dataset(
            org_id,
            &definition.name,
            StreamType::Traces,
            PhysicalDatasetKind::Raw,
            range,
        )
        .await?;
    if raw_files.is_empty() {
        return Ok(Vec::new());
    }
    let summary_files = state
        .storage
        .parquet_file_meta
        .find_dataset(
            org_id,
            &definition.name,
            StreamType::Traces,
            PhysicalDatasetKind::TraceSummary,
            range,
        )
        .await?;
    if summary_files.is_empty() {
        return Ok(Vec::new());
    }

    let ctx = SessionContext::new();
    let object_store_url = ObjectStoreUrl::parse("molesignal://trace-list")
        .map_err(|error| Error::internal(format!("trace-list object store URL: {error}")))?;
    ctx.runtime_env().register_object_store(
        object_store_url.as_ref(),
        state.storage.object_store.clone(),
    );

    let raw_table = PrunedParquetTable::new(
        arrow_schema::to_arrow(&definition.schema),
        &raw_files,
        object_store_url.clone(),
        range,
        Some(PhysicalDatasetKind::Raw),
    );
    ctx.register_table(TableReference::bare(RAW_TABLE), Arc::new(raw_table))
        .map_err(|error| Error::internal(format!("register trace Span table: {error}")))?;

    let summary_definition = crate::infra::ingester::physical_schema::project(
        definition,
        PhysicalDatasetKind::TraceSummary,
    );
    let summary_table = PrunedParquetTable::new(
        arrow_schema::to_arrow(&summary_definition.schema),
        &summary_files,
        object_store_url,
        range,
        Some(PhysicalDatasetKind::TraceSummary),
    );
    ctx.register_table(TableReference::bare(SUMMARY_TABLE), Arc::new(summary_table))
        .map_err(|error| Error::internal(format!("register trace summary table: {error}")))?;

    let sql = build_sql(context, fetch_limit)?;
    let batches = ctx
        .sql(&sql)
        .await
        .map_err(|error| Error::internal(format!("plan filtered trace summaries: {error}")))?
        .collect()
        .await
        .map_err(|error| Error::internal(format!("scan filtered trace summaries: {error}")))?;
    let rows = rows_from_batches(&batches);
    tracing::debug!(
        raw_files = raw_files.len(),
        summary_files = summary_files.len(),
        returned = rows.len(),
        "trace Span filters joined to summary projection"
    );
    Ok(rows)
}

fn build_sql(context: &TraceListContext, fetch_limit: usize) -> Result<String> {
    let from_ns = context
        .from
        .checked_mul(1_000)
        .ok_or_else(|| Error::invalid("trace time_from is out of range"))?;
    let to_ns = context
        .to
        .checked_mul(1_000)
        .ok_or_else(|| Error::invalid("trace time_to is out of range"))?;
    let raw_filters = context
        .filters
        .iter()
        .filter(|filter| filter.is_span_filter())
        .collect::<Vec<_>>();
    if raw_filters.is_empty() {
        return Err(Error::internal(
            "trace Span-filter query requires at least one Span filter",
        ));
    }
    let having = raw_filters
        .into_iter()
        .map(|filter| {
            Ok(format!(
                "MAX(CASE WHEN {} THEN 1 ELSE 0 END) = 1",
                filter_predicate("r", filter, &filter.field)?
            ))
        })
        .collect::<Result<Vec<_>>>()?;

    let start = qualified("s", TRACE_SUMMARY_START_NS_FIELD);
    let duration = qualified("s", TRACE_SUMMARY_DURATION_NS_FIELD);
    let span_count = qualified("s", TRACE_SUMMARY_SPAN_COUNT_FIELD);
    let error_count = qualified("s", TRACE_SUMMARY_ERROR_COUNT_FIELD);
    let trace_id = qualified("s", "trace_id");
    let mut predicates = vec![format!("{} = 1", qualified("s", SUMMARY_RANK_FIELD))];
    for filter in context
        .filters
        .iter()
        .filter(|filter| !filter.is_span_filter())
    {
        let column = filter
            .summary_column()
            .ok_or_else(|| Error::internal("invalid trace summary filter"))?;
        predicates.push(filter_predicate("s", filter, column)?);
    }
    if let Some(query) = context.q.as_deref() {
        let value = sql_literal(&format!("%{}%", query.to_ascii_lowercase()));
        let fields = ["trace_id", "span_id", "service.name", "name", "status_code"];
        predicates.push(format!(
            "({})",
            fields
                .into_iter()
                .map(|field| format!(
                    "LOWER(COALESCE(CAST({} AS VARCHAR), '')) LIKE {value}",
                    qualified("s", field)
                ))
                .collect::<Vec<_>>()
                .join(" OR ")
        ));
    }
    if let Some(boundary) = context.boundary.as_ref() {
        predicates.push(cursor_predicate(context, boundary.direction));
    }
    let order = order_by(context);

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
           SELECT \"trace_id\"\n\
           FROM \"{raw_table}\" AS r\n\
           WHERE \"trace_id\" IS NOT NULL\n\
           GROUP BY \"trace_id\"\n\
           HAVING {having}\n\
         )\n\
         SELECT {trace_id} AS trace_id,\n\
                {} AS service,\n\
                {} AS operation,\n\
                {start} AS start_ns,\n\
                {duration} AS duration_ns,\n\
                {span_count} AS span_count,\n\
                {error_count} AS error_count\n\
         FROM ranked_summaries AS s\n\
         INNER JOIN matching_trace_ids AS m\n\
           ON {trace_id} = {}\n\
         WHERE {}\n\
         ORDER BY {order}\n\
         LIMIT {fetch_limit}",
        qualified("s", "service.name"),
        qualified("s", "name"),
        qualified("m", "trace_id"),
        predicates.join(" AND "),
        start_field = TRACE_SUMMARY_START_NS_FIELD,
        rank_field = SUMMARY_RANK_FIELD,
        summary_table = SUMMARY_TABLE,
        raw_table = RAW_TABLE,
        having = having.join(" AND "),
    ))
}

fn filter_predicate(alias: &str, filter: &TraceFilter, field: &str) -> Result<String> {
    let column = qualified(alias, field);
    let expression = match filter.data_type {
        FieldType::Timestamp => format!("CAST({column} AS BIGINT)"),
        _ => column,
    };
    if filter.op == "contains" {
        return Ok(format!(
            "CAST({expression} AS VARCHAR) LIKE {}",
            sql_literal(&format!("%{}%", filter.value))
        ));
    }
    let value = match filter.data_type {
        FieldType::Utf8 => sql_literal(&filter.value),
        FieldType::Int64 | FieldType::Float64 | FieldType::Timestamp => filter.value.clone(),
        FieldType::Bool => filter.value.to_ascii_uppercase(),
        FieldType::Json => {
            return Err(Error::invalid(format!(
                "trace field `{field}` requires a JSON path query in SQL mode"
            )));
        }
    };
    Ok(format!("{expression} {} {value}", filter.op))
}

fn cursor_predicate(context: &TraceListContext, direction: CursorDirection) -> String {
    let position = &context
        .boundary
        .as_ref()
        .expect("boundary checked")
        .position;
    let fields = match context.sort {
        TraceListSort::Latest => vec![
            (
                qualified("s", TRACE_SUMMARY_START_NS_FIELD),
                CursorValue::Integer(position.start_ns),
                CursorSortDirection::Desc,
            ),
            (
                qualified("s", "trace_id"),
                CursorValue::Text(position.trace_id.clone()),
                CursorSortDirection::Desc,
            ),
        ],
        TraceListSort::Earliest => vec![
            (
                qualified("s", TRACE_SUMMARY_START_NS_FIELD),
                CursorValue::Integer(position.start_ns),
                CursorSortDirection::Asc,
            ),
            (
                qualified("s", "trace_id"),
                CursorValue::Text(position.trace_id.clone()),
                CursorSortDirection::Asc,
            ),
        ],
        sort => {
            let primary = match sort {
                TraceListSort::DurationDesc | TraceListSort::DurationAsc => {
                    TRACE_SUMMARY_DURATION_NS_FIELD
                }
                TraceListSort::SpanCountDesc => TRACE_SUMMARY_SPAN_COUNT_FIELD,
                TraceListSort::ErrorsDesc => TRACE_SUMMARY_ERROR_COUNT_FIELD,
                TraceListSort::Latest | TraceListSort::Earliest => unreachable!(),
            };
            let primary_direction = if sort == TraceListSort::DurationAsc {
                CursorSortDirection::Asc
            } else {
                CursorSortDirection::Desc
            };
            vec![
                (
                    qualified("s", primary),
                    CursorValue::Integer(position.primary),
                    primary_direction,
                ),
                (
                    qualified("s", TRACE_SUMMARY_START_NS_FIELD),
                    CursorValue::Integer(position.start_ns),
                    CursorSortDirection::Desc,
                ),
                (
                    qualified("s", "trace_id"),
                    CursorValue::Text(position.trace_id.clone()),
                    CursorSortDirection::Desc,
                ),
            ]
        }
    };
    let borrowed = fields
        .iter()
        .map(|(name, value, order)| (name.as_str(), value.clone(), *order))
        .collect::<Vec<_>>();
    lexicographic_seek(&borrowed, direction, sql_literal)
}

fn order_by(context: &TraceListContext) -> String {
    let fields = match context.sort {
        TraceListSort::Latest => vec![
            (TRACE_SUMMARY_START_NS_FIELD, CursorSortDirection::Desc),
            ("trace_id", CursorSortDirection::Desc),
        ],
        TraceListSort::Earliest => vec![
            (TRACE_SUMMARY_START_NS_FIELD, CursorSortDirection::Asc),
            ("trace_id", CursorSortDirection::Asc),
        ],
        TraceListSort::DurationDesc => vec![
            (TRACE_SUMMARY_DURATION_NS_FIELD, CursorSortDirection::Desc),
            (TRACE_SUMMARY_START_NS_FIELD, CursorSortDirection::Desc),
            ("trace_id", CursorSortDirection::Desc),
        ],
        TraceListSort::DurationAsc => vec![
            (TRACE_SUMMARY_DURATION_NS_FIELD, CursorSortDirection::Asc),
            (TRACE_SUMMARY_START_NS_FIELD, CursorSortDirection::Desc),
            ("trace_id", CursorSortDirection::Desc),
        ],
        TraceListSort::SpanCountDesc => vec![
            (TRACE_SUMMARY_SPAN_COUNT_FIELD, CursorSortDirection::Desc),
            (TRACE_SUMMARY_START_NS_FIELD, CursorSortDirection::Desc),
            ("trace_id", CursorSortDirection::Desc),
        ],
        TraceListSort::ErrorsDesc => vec![
            (TRACE_SUMMARY_ERROR_COUNT_FIELD, CursorSortDirection::Desc),
            (TRACE_SUMMARY_START_NS_FIELD, CursorSortDirection::Desc),
            ("trace_id", CursorSortDirection::Desc),
        ],
    };
    let reverse = context
        .boundary
        .as_ref()
        .is_some_and(|boundary| boundary.direction == CursorDirection::Before);
    fields
        .into_iter()
        .map(|(field, direction)| {
            let direction = match (direction, reverse) {
                (CursorSortDirection::Asc, false) | (CursorSortDirection::Desc, true) => "ASC",
                (CursorSortDirection::Desc, false) | (CursorSortDirection::Asc, true) => "DESC",
            };
            format!("{} {direction}", qualified("s", field))
        })
        .collect::<Vec<_>>()
        .join(", ")
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
            rows.push(TraceListRow {
                item: super::TraceListItem {
                    trace_id: trace_id.to_string(),
                    service: string_at(batch, "service", row)
                        .unwrap_or_default()
                        .to_string(),
                    operation: string_at(batch, "operation", row)
                        .unwrap_or_default()
                        .to_string(),
                    start_ns: integer_at(batch, "start_ns", row).unwrap_or_default(),
                    duration_ms: duration_ns as f64 / 1_000_000.0,
                    span_count: integer_at(batch, "span_count", row).unwrap_or_default(),
                    error_count: integer_at(batch, "error_count", row).unwrap_or_default(),
                },
                duration_ns,
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

    use super::*;
    use crate::{domain::stream::FieldDef, shared::trace::normalization::canonical_trace_schema};

    fn context() -> TraceListContext {
        let mut schema = canonical_trace_schema();
        schema.fields.push(FieldDef {
            name: "http.status_code".into(),
            data_type: FieldType::Int64,
            nullable: true,
            indexed: false,
            encrypted: false,
            exact: false,
        });
        let filters = super::super::filter::parse(
            Some(
                &serde_json::json!([
                    {"field": "http.status_code", "op": ">=", "value": "500"},
                    {"field": "span_count", "op": ">=", "value": "2"}
                ])
                .to_string(),
            ),
            &schema,
            32,
        )
        .unwrap();
        TraceListContext {
            from: 0,
            to: 1_000,
            sort: TraceListSort::Latest,
            page_size: 20,
            q: None,
            filters,
            boundary: None,
        }
    }

    #[tokio::test]
    async fn any_span_match_returns_values_from_the_summary_row() {
        let ctx = SessionContext::new();
        let raw_schema = Arc::new(ArrowSchema::new(vec![
            Field::new("trace_id", DataType::Utf8, false),
            Field::new("http.status_code", DataType::Int64, true),
        ]));
        let raw = RecordBatch::try_new(
            raw_schema.clone(),
            vec![
                Arc::new(StringArray::from(vec!["trace-a", "trace-b", "trace-b"])),
                Arc::new(Int64Array::from(vec![200, 200, 503])),
            ],
        )
        .unwrap();
        ctx.register_table(
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
            Field::new(TRACE_SUMMARY_START_NS_FIELD, DataType::Int64, false),
            Field::new(TRACE_SUMMARY_DURATION_NS_FIELD, DataType::Int64, false),
            Field::new(TRACE_SUMMARY_SPAN_COUNT_FIELD, DataType::Int64, false),
            Field::new(TRACE_SUMMARY_ERROR_COUNT_FIELD, DataType::Int64, false),
        ]));
        let summary = RecordBatch::try_new(
            summary_schema.clone(),
            vec![
                Arc::new(StringArray::from(vec!["trace-a", "trace-b"])),
                Arc::new(StringArray::from(vec!["a-root", "b-root"])),
                Arc::new(StringArray::from(vec![None::<&str>, None])),
                Arc::new(StringArray::from(vec!["web", "checkout"])),
                Arc::new(StringArray::from(vec!["GET /", "POST /checkout"])),
                Arc::new(StringArray::from(vec!["OK", "ERROR"])),
                Arc::new(Int64Array::from(vec![100, 200])),
                Arc::new(Int64Array::from(vec![10, 20])),
                Arc::new(Int64Array::from(vec![1, 2])),
                Arc::new(Int64Array::from(vec![0, 1])),
            ],
        )
        .unwrap();
        ctx.register_table(
            SUMMARY_TABLE,
            Arc::new(MemTable::try_new(summary_schema, vec![vec![summary]]).unwrap()),
        )
        .unwrap();

        let batches = ctx
            .sql(&build_sql(&context(), 21).unwrap())
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();
        let rows = rows_from_batches(&batches);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].item.trace_id, "trace-b");
        assert_eq!(rows[0].item.service, "checkout");
        assert_eq!(rows[0].item.span_count, 2);
    }

    #[test]
    fn generated_sql_uses_having_for_any_span_and_summary_predicates_for_counts() {
        let sql = build_sql(&context(), 21).unwrap();
        assert!(sql.contains("MAX(CASE WHEN r.\"http.status_code\" >= 500 THEN 1 ELSE 0 END) = 1"));
        assert!(sql.contains("s.\"molesignal.trace.span_count\" >= 2"));
        assert!(sql.contains("LIMIT 21"));
    }
}
