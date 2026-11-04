// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How many traces a Span-filter query matches, bar by bar of the explorer
//! histogram.
//!
//! The statement joins the summaries to the traces that have a matching Span,
//! with the conditions the list joins them by, and counts where the list
//! would sort and cut a page. Unlike the list it does not rank the summaries
//! of a trace first: a summary that was written twice counts twice.

use serde_json::Value;

use super::{SUMMARY_TABLE, TraceListContext, plan, qualified, register_tables};
use crate::{
    api::{
        AppState,
        http::routes::web::{
            histogram::{
                BUCKET_COLUMN, BarLatency, BucketPlan, Counts, ERRORS_COLUMN, TOTAL_COLUMN,
                read_counts, within_scan_limit,
            },
            trace::list::scan::count::CountedBars,
        },
    },
    domain::stream::StreamDefinition,
    infra::query::arrow_json::batches_to_json,
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
        trace::summary::{
            TRACE_SUMMARY_DURATION_NS_FIELD, TRACE_SUMMARY_ERROR_COUNT_FIELD,
            TRACE_SUMMARY_START_NS_FIELD,
        },
    },
};

/// Result columns of how long a bar's traces took: names no field has.
const P50_COLUMN: &str = "__histogram_p50";
const P95_COLUMN: &str = "__histogram_p95";
const P99_COLUMN: &str = "__histogram_p99";

/// The bars of `plan` that hold a trace the query matches, with how long
/// their traces took. `None` when the range holds more spans and summaries
/// than one query may read.
pub(in super::super) async fn bars(
    state: &AppState,
    definition: &StreamDefinition,
    context: &TraceListContext,
    plan: &BucketPlan,
) -> Result<Option<CountedBars>> {
    let range = TimeRange::new(TimestampMicros(context.from), TimestampMicros(context.to));

    let Some(tables) = register_tables(state, definition, range).await? else {
        return Ok(Some((Vec::new(), Vec::new())));
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
        Counts::Bars { bars, .. } => Ok(Some((bars, read_latency(&columns, &rows)))),
        // Nothing masks a statement this module runs itself.
        Counts::TimeMasked => Err(Error::internal("trace histogram returned no bars")),
    }
}

/// How long each bar's traces took, as the statement read it.
fn read_latency(columns: &[String], rows: &[Vec<Value>]) -> Vec<BarLatency> {
    let position = |name: &str| columns.iter().position(|column| column == name);
    let number = |row: &[Value], index: Option<usize>| {
        index
            .and_then(|index| row.get(index))
            .and_then(Value::as_f64)
            .map(|value| value.round() as i64)
    };

    let (bucket, p50, p95, p99) = (
        position(BUCKET_COLUMN),
        position(P50_COLUMN),
        position(P95_COLUMN),
        position(P99_COLUMN),
    );

    rows.iter()
        .filter_map(|row| {
            Some(BarLatency {
                index: number(row, bucket)?,
                p50_ns: number(row, p50)?,
                p95_ns: number(row, p95)?,
                p99_ns: number(row, p99)?,
            })
        })
        .collect()
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
    let duration = format!(
        "CAST({} AS DOUBLE)",
        qualified("s", TRACE_SUMMARY_DURATION_NS_FIELD)
    );
    let percentile = |share: f64, alias: &str| {
        format!("approx_percentile_cont({share}) WITHIN GROUP (ORDER BY {duration}) AS {alias}")
    };

    let asked = plan::plan(context)?;

    let mut predicates = vec![
        format!("{start} >= {from_ns}"),
        format!("{start} < {to_ns}"),
    ];

    predicates.extend(asked.predicates);

    Ok(format!(
        "WITH matching_trace_ids AS (\n\
           {}\n\
         )\n\
         SELECT {} AS {BUCKET_COLUMN},\n\
                COUNT(*) AS {TOTAL_COLUMN},\n\
                SUM(CASE WHEN {error_count} > 0 THEN 1 ELSE 0 END) AS {ERRORS_COLUMN},\n\
                {}, {}, {}\n\
         FROM \"{SUMMARY_TABLE}\" AS s\n\
         INNER JOIN matching_trace_ids AS m\n\
           ON {} = {}\n\
         WHERE {}\n\
         GROUP BY 1",
        asked.matching,
        // A trace begins on a nanosecond; a bar is counted in microseconds.
        plan.bucket_sql(&format!("{start} / 1000")),
        percentile(0.5, P50_COLUMN),
        percentile(0.95, P95_COLUMN),
        percentile(0.99, P99_COLUMN),
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
        api::http::routes::web::{
            histogram::BarCount,
            trace::list::{TraceListSort, filter, span_filter::RAW_TABLE},
        },
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
            condition: None,
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
            Field::new(TRACE_SUMMARY_DURATION_NS_FIELD, DataType::Int64, false),
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
                Arc::new(Int64Array::from(vec![
                    5_000_000, 40_000_000, 2_000_000, 7_000_000, 9_000_000,
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

    async fn rows(context: &TraceListContext) -> (Vec<String>, Vec<Vec<Value>>) {
        let batches = session()
            .await
            .sql(&count_sql(context, &plan()).expect("sql"))
            .await
            .expect("the statement should plan")
            .collect()
            .await
            .expect("the statement should execute");

        batches_to_json(&batches)
    }

    async fn counted(context: &TraceListContext) -> Vec<BarCount> {
        let (columns, rows) = rows(context).await;

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
    async fn a_trace_that_leaves_a_value_out_is_counted_when_none_of_its_spans_has_it() {
        let context = context(
            None,
            r#"[{"field":"http.status_code","op":"!=","value":"503"}]"#,
        );

        // `b` began inside the range with a Span that answered 503 beside one
        // that did not; `c` and `d` have no such Span.
        assert_eq!(counted(&context).await, vec![bar(1, 1, 0), bar(2, 1, 0)]);
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
        let asked = super::plan::plan(&context).expect("plan");

        assert!(statement.contains(&asked.matching));
        assert!(
            statement
                .contains("MAX(CASE WHEN r.\"http.status_code\" >= 500 THEN 1 ELSE 0 END) = 1")
        );

        for predicate in asked.predicates {
            assert!(statement.contains(&predicate));
        }

        assert!(statement.contains("s.\"molesignal.trace.start_ns\" >= 10500000"));
        assert!(statement.contains("s.\"molesignal.trace.start_ns\" < 13000000"));
        assert!(statement.contains(
            "(s.\"molesignal.trace.start_ns\" / 1000 - 10000) / 1000 AS __histogram_bucket"
        ));
    }

    #[tokio::test]
    async fn reads_how_long_the_traces_of_each_bar_took() {
        let context = context(
            None,
            r#"[{"field":"http.status_code","op":">=","value":"200"}]"#,
        );
        let (columns, rows) = rows(&context).await;
        let mut latency = read_latency(&columns, &rows);
        latency.sort_by_key(|took| took.index);

        // `b` (40 ms) is the first bar's only trace, `c` (2 ms) the second's, `d` (7 ms) the third's.
        assert_eq!(
            latency
                .iter()
                .map(|took| (took.index, took.p50_ns))
                .collect::<Vec<_>>(),
            vec![(0, 40_000_000), (1, 2_000_000), (2, 7_000_000)]
        );
        assert!(latency.iter().all(|took| took.p50_ns <= took.p99_ns));
    }
}
