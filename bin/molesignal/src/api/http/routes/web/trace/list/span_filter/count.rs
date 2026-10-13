// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How many traces a Span-filter query matches, bar by bar of the explorer
//! histogram.
//!
//! The statement joins the summaries to the traces that have a matching Span,
//! with the conditions the list joins them by, and counts where the list
//! would sort and cut a page. Unlike the list it does not rank the summaries
//! of a trace first: a summary that was written twice counts twice.

use super::{
    SUMMARY_TABLE, TraceListContext, matching_trace_ids_sql, qualified, register_tables,
    summary_predicates,
};
use crate::{
    api::{
        AppState,
        http::routes::web::histogram::{
            BUCKET_COLUMN, BarCount, BucketPlan, Counts, ERRORS_COLUMN, TOTAL_COLUMN, read_counts,
            within_scan_limit,
        },
    },
    domain::stream::StreamDefinition,
    infra::query::arrow_json::batches_to_json,
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
        trace::summary::{TRACE_SUMMARY_ERROR_COUNT_FIELD, TRACE_SUMMARY_START_NS_FIELD},
    },
};

/// The bars of `plan` that hold a trace the query matches. `None` when the
/// range holds more spans and summaries than one query may read.
pub(in super::super) async fn bars(
    state: &AppState,
    definition: &StreamDefinition,
    context: &TraceListContext,
    plan: &BucketPlan,
) -> Result<Option<Vec<BarCount>>> {
    let range = TimeRange::new(TimestampMicros(context.from), TimestampMicros(context.to));

    let Some(tables) = register_tables(state, definition, range).await? else {
        return Ok(Some(Vec::new()));
    };

    if !within_scan_limit(tables.candidate_rows) {
        return Ok(None);
    }

    let statement = count_sql(context, plan)?;

    let batches = tables
        .session
        .sql(&statement)
        .await
        .map_err(|error| Error::internal(format!("plan trace histogram: {error}")))?
        .collect()
        .await
        .map_err(|error| Error::internal(format!("scan trace histogram: {error}")))?;

    let (columns, rows) = batches_to_json(&batches);

    match read_counts(&columns, &rows)? {
        Counts::Bars { bars, .. } => Ok(Some(bars)),
        // Nothing masks a statement this module runs itself.
        Counts::TimeMasked => Err(Error::internal("trace histogram returned no bars")),
    }
}

fn count_sql(context: &TraceListContext, plan: &BucketPlan) -> Result<String> {
    let from_ns = context
        .from
        .checked_mul(1_000)
        .ok_or_else(|| Error::invalid("trace time_from is out of range"))?;

    let to_ns = context
        .to
        .checked_mul(1_000)
        .ok_or_else(|| Error::invalid("trace time_to is out of range"))?;

    let start = qualified("s", TRACE_SUMMARY_START_NS_FIELD);
    let error_count = qualified("s", TRACE_SUMMARY_ERROR_COUNT_FIELD);
    let mut predicates = vec![
        format!("{start} >= {from_ns}"),
        format!("{start} < {to_ns}"),
    ];
    predicates.extend(summary_predicates(context)?);

    Ok(format!(
        "WITH matching_trace_ids AS (\n\
           {}\n\
         )\n\
         SELECT {} AS {BUCKET_COLUMN},\n\
                COUNT(*) AS {TOTAL_COLUMN},\n\
                SUM(CASE WHEN {error_count} > 0 THEN 1 ELSE 0 END) AS {ERRORS_COLUMN}\n\
         FROM \"{SUMMARY_TABLE}\" AS s\n\
         INNER JOIN matching_trace_ids AS m\n\
           ON {} = {}\n\
         WHERE {}\n\
         GROUP BY 1",
        matching_trace_ids_sql(context)?,
        // A trace begins on a nanosecond; a bar is counted in microseconds.
        plan.bucket_sql(&format!("{start} / 1000")),
        qualified("s", "trace_id"),
        qualified("m", "trace_id"),
        predicates.join(" AND "),
    ))
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
    use crate::{
        api::http::routes::web::trace::list::{TraceListSort, filter, span_filter::RAW_TABLE},
        domain::stream::{FieldDef, FieldType},
        shared::trace::{
            normalization::canonical_trace_schema, summary::TRACE_SUMMARY_SPAN_COUNT_FIELD,
        },
    };

    /// Bars of a millisecond from 10 ms, holding what began in `[10.5 ms, 13 ms)`.
    fn plan() -> BucketPlan {
        BucketPlan::new(Some(10_500), Some(13_000), Some(1_000), Some(10_000)).expect("plan")
    }

    fn context(q: Option<&str>, filters: &str) -> TraceListContext {
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

        TraceListContext {
            from: 10_500,
            to: 13_000,
            sort: TraceListSort::Latest,
            page_size: 20,
            q: q.map(String::from),
            filters: filter::parse(Some(filters), &schema, 32).expect("filters"),
            boundary: None,
        }
    }

    /// Five traces. `a` began before the range and `e` at its end; `b`, `c`
    /// and `d` began inside it. Every trace but `c` has a Span that failed
    /// with a 5xx.
    async fn session() -> SessionContext {
        let session = SessionContext::new();

        let raw_schema = Arc::new(ArrowSchema::new(vec![
            Field::new("trace_id", DataType::Utf8, false),
            Field::new("http.status_code", DataType::Int64, true),
        ]));
        let raw = RecordBatch::try_new(
            raw_schema.clone(),
            vec![
                Arc::new(StringArray::from(vec!["a", "b", "b", "c", "d", "e"])),
                Arc::new(Int64Array::from(vec![500, 200, 503, 200, 502, 500])),
            ],
        )
        .expect("spans");
        session
            .register_table(
                RAW_TABLE,
                Arc::new(MemTable::try_new(raw_schema, vec![vec![raw]]).expect("span table")),
            )
            .expect("register spans");

        let summary_schema = Arc::new(ArrowSchema::new(vec![
            Field::new("trace_id", DataType::Utf8, false),
            Field::new("span_id", DataType::Utf8, true),
            Field::new("service.name", DataType::Utf8, true),
            Field::new("name", DataType::Utf8, true),
            Field::new("status_code", DataType::Utf8, true),
            Field::new(TRACE_SUMMARY_START_NS_FIELD, DataType::Int64, false),
            Field::new(TRACE_SUMMARY_SPAN_COUNT_FIELD, DataType::Int64, false),
            Field::new(TRACE_SUMMARY_ERROR_COUNT_FIELD, DataType::Int64, false),
        ]));
        let summary = RecordBatch::try_new(
            summary_schema.clone(),
            vec![
                Arc::new(StringArray::from(vec!["a", "b", "c", "d", "e"])),
                Arc::new(StringArray::from(vec!["a1", "b1", "c1", "d1", "e1"])),
                Arc::new(StringArray::from(vec![
                    "web", "checkout", "web", "web", "web",
                ])),
                Arc::new(StringArray::from(vec!["GET /"; 5])),
                Arc::new(StringArray::from(vec![
                    "ERROR", "ERROR", "OK", "OK", "ERROR",
                ])),
                // In nanoseconds.
                Arc::new(Int64Array::from(vec![
                    10_499_999, 10_500_000, 11_200_000, 12_999_999, 13_000_000,
                ])),
                Arc::new(Int64Array::from(vec![1, 2, 1, 1, 1])),
                Arc::new(Int64Array::from(vec![1, 1, 0, 0, 1])),
            ],
        )
        .expect("summaries");
        session
            .register_table(
                SUMMARY_TABLE,
                Arc::new(
                    MemTable::try_new(summary_schema, vec![vec![summary]]).expect("summary table"),
                ),
            )
            .expect("register summaries");

        session
    }

    async fn counted(context: &TraceListContext) -> Vec<BarCount> {
        let batches = session()
            .await
            .sql(&count_sql(context, &plan()).expect("sql"))
            .await
            .expect("the statement should plan")
            .collect()
            .await
            .expect("the statement should execute");
        let (columns, rows) = batches_to_json(&batches);

        match read_counts(&columns, &rows).expect("counts") {
            Counts::Bars { mut bars, split } => {
                assert!(split);
                bars.sort_by_key(|bar| bar.index);
                bars
            }
            Counts::TimeMasked => panic!("time is not masked"),
        }
    }

    fn bar(index: i64, total: u64, errors: u64) -> BarCount {
        BarCount {
            index,
            total,
            errors,
        }
    }

    #[tokio::test]
    async fn traces_with_a_matching_span_are_counted_into_the_bar_they_began_in() {
        let context = context(
            None,
            r#"[{"field":"http.status_code","op":">=","value":"500"}]"#,
        );

        // `a` and `e` have a matching Span but began outside the range, and
        // `c` began inside it without one.
        assert_eq!(counted(&context).await, vec![bar(0, 1, 1), bar(2, 1, 0)]);
    }

    #[tokio::test]
    async fn summary_filters_and_free_text_narrow_what_is_counted() {
        let by_spans = context(
            None,
            r#"[{"field":"http.status_code","op":">=","value":"500"},
                {"field":"span_count","op":">=","value":"2"}]"#,
        );
        assert_eq!(counted(&by_spans).await, vec![bar(0, 1, 1)]);

        let by_text = context(
            Some("WEB"),
            r#"[{"field":"http.status_code","op":">=","value":"500"}]"#,
        );
        assert_eq!(counted(&by_text).await, vec![bar(2, 1, 0)]);
    }

    #[test]
    fn the_statement_joins_with_the_conditions_of_the_list() {
        let context = context(
            Some("web"),
            r#"[{"field":"http.status_code","op":">=","value":"500"},
                {"field":"span_count","op":">=","value":"2"}]"#,
        );
        let statement = count_sql(&context, &plan()).expect("sql");

        assert!(statement.contains(&matching_trace_ids_sql(&context).expect("spans")));
        assert!(
            statement
                .contains("MAX(CASE WHEN r.\"http.status_code\" >= 500 THEN 1 ELSE 0 END) = 1")
        );

        for predicate in summary_predicates(&context).expect("predicates") {
            assert!(statement.contains(&predicate));
        }

        assert!(statement.contains("s.\"molesignal.trace.start_ns\" >= 10500000"));
        assert!(statement.contains("s.\"molesignal.trace.start_ns\" < 13000000"));
        assert!(statement.contains(
            "(s.\"molesignal.trace.start_ns\" / 1000 - 10000) / 1000 AS __histogram_bucket"
        ));
    }
}
