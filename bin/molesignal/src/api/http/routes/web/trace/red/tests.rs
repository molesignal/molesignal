// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The statements run against a table of Spans the way the engine runs them,
//! and the figures read out of what they return.

use std::sync::Arc;

use arrow::{
    array::{ArrayRef, Int64Array, RecordBatch, StringArray, TimestampMicrosecondArray},
    datatypes::{DataType, Field, Schema as ArrowSchema, TimeUnit},
};
use datafusion::{datasource::MemTable, prelude::SessionContext};
use serde_json::{Value, json};

use super::{
    RedScope, RedUnavailable,
    reading::{self, Figures, Overview},
    settled,
    statements::{self, Grouping, Source},
};
use crate::{
    api::http::routes::web::{histogram::BucketPlan, trace::filter},
    domain::{
        query::QueryResult,
        stream::{FieldDef, FieldType, StreamDefinition, StreamType},
    },
    infra::query::{arrow_json::batches_to_json, scan_limit::ensure_within_scan_limit},
    shared::{
        Error, Result, ids::Id, time::TimestampMicros, trace::normalization::canonical_trace_schema,
    },
};

const SECOND: i64 = 1_000_000;
/// Ten minutes past, where the first bar begins.
const ORIGIN: i64 = 600 * SECOND;
const MILLISECOND_NS: i64 = 1_000_000;

/// Three minutes in bars of one.
fn plan() -> BucketPlan {
    BucketPlan::new(
        Some(ORIGIN),
        Some(ORIGIN + 180 * SECOND),
        Some(60 * SECOND),
        Some(ORIGIN),
    )
    .expect("plan")
}

fn definition() -> StreamDefinition {
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

    StreamDefinition {
        id: Id::from_string("spans"),
        org_id: Id::from_string("org-1"),
        name: "spans".into(),
        stream_type: StreamType::TRACES,
        schema,
        retention: None,
        created_at: TimestampMicros(1),
        updated_at: TimestampMicros(1),
    }
}

/// Three traces.
///
/// `a`: `checkout` serves `GET /cart` (a server root, 120 ms), calls out
/// (a client Span) to `inventory`, whose server Span fails with a 500.
/// `b`: an unspecified root of `checkout`, a second long, with an internal Span.
/// `c`: a consumer root of `mailer` that fails, in the third bar.
fn session() -> SessionContext {
    let schema = Arc::new(ArrowSchema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, None),
            false,
        ),
        Field::new("trace_id", DataType::Utf8, false),
        Field::new("span_id", DataType::Utf8, false),
        Field::new("parent_span_id", DataType::Utf8, true),
        Field::new("name", DataType::Utf8, true),
        Field::new("kind", DataType::Int64, true),
        Field::new("duration_ns", DataType::Int64, true),
        Field::new("status_code", DataType::Utf8, true),
        Field::new("service.name", DataType::Utf8, true),
        Field::new("http.status_code", DataType::Int64, true),
    ]));

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(TimestampMicrosecondArray::from(vec![
                ORIGIN + 10 * SECOND,
                ORIGIN + 11 * SECOND,
                ORIGIN + 12 * SECOND,
                ORIGIN + 70 * SECOND,
                ORIGIN + 71 * SECOND,
                ORIGIN + 130 * SECOND,
            ])),
            Arc::new(StringArray::from(vec!["a", "a", "a", "b", "b", "c"])),
            Arc::new(StringArray::from(vec!["a1", "a2", "a3", "b1", "b2", "c1"])),
            Arc::new(StringArray::from(vec![
                Some(""),
                Some("a1"),
                Some("a2"),
                Some(""),
                Some("b1"),
                None,
            ])),
            Arc::new(StringArray::from(vec![
                "GET /cart",
                "SELECT",
                "GET /stock",
                "GET /cart",
                "render",
                "send",
            ])),
            Arc::new(Int64Array::from(vec![2, 3, 2, 0, 1, 5])),
            Arc::new(Int64Array::from(vec![
                120 * MILLISECOND_NS,
                20 * MILLISECOND_NS,
                15 * MILLISECOND_NS,
                1_000 * MILLISECOND_NS,
                5 * MILLISECOND_NS,
                2 * MILLISECOND_NS,
            ])),
            Arc::new(StringArray::from(vec![
                "OK", "UNSET", "ERROR", "OK", "UNSET", "ERROR",
            ])),
            Arc::new(StringArray::from(vec![
                "checkout",
                "checkout",
                "inventory",
                "checkout",
                "checkout",
                "mailer",
            ])),
            Arc::new(Int64Array::from(vec![
                Some(200),
                None,
                Some(500),
                Some(200),
                None,
                None,
            ])),
        ],
    )
    .expect("spans");

    let session = SessionContext::new();

    session
        .register_table(
            "spans",
            Arc::new(MemTable::try_new(schema, vec![vec![batch]]).expect("table")),
        )
        .expect("register spans");

    session
}

async fn execute(session: SessionContext, statement: String) -> Result<QueryResult> {
    let batches = session
        .sql(&statement)
        .await
        .map_err(|error| Error::internal(format!("plan `{statement}`: {error}")))?
        .collect()
        .await
        .map_err(|error| Error::internal(format!("run `{statement}`: {error}")))?;

    let (columns, rows) = batches_to_json(&batches);

    Ok(QueryResult {
        columns,
        rows,
        scanned_rows: 0,
        took_ms: 0,
        federation: None,
    })
}

fn source(scope: RedScope, filters: &str, text: Option<&str>) -> Source {
    source_where(scope, filters, text, None)
}

/// The Spans of `scope` that pass the filters, the text and a condition tree.
fn source_where(scope: RedScope, filters: &str, text: Option<&str>, tree: Option<Value>) -> Source {
    let definition = definition();
    let filters = filter::parse(Some(filters), &definition.schema, 32).expect("filters");

    let tree = tree.map(|tree| {
        filter::condition::parse(Some(&tree.to_string()), &definition.schema, 32)
            .expect("condition")
            .expect("a condition")
    });

    let query = filter::TraceCondition::of(&filters, text, tree.as_ref());

    Source::new(&definition, scope, &query).expect("source")
}

async fn overview(source: &Source) -> Overview {
    let session = session();

    reading::overview(source, &plan(), |statement| {
        execute(session.clone(), statement)
    })
    .await
    .expect("overview")
}

async fn requests(scope: RedScope, filters: &str, text: Option<&str>) -> u64 {
    overview(&source(scope, filters, text)).await.total.requests
}

fn grouping(field: &str) -> Grouping {
    let definition = definition();

    let declared = definition
        .schema
        .fields
        .iter()
        .find(|candidate| candidate.name == field)
        .expect("field");

    Grouping::new(declared).expect("grouping")
}

fn result(columns: &[&str], rows: Vec<Vec<Value>>) -> QueryResult {
    QueryResult {
        columns: columns.iter().map(|column| column.to_string()).collect(),
        rows,
        scanned_rows: 0,
        took_ms: 0,
        federation: None,
    }
}

#[tokio::test]
async fn entry_spans_are_where_a_service_took_a_request_in() {
    // The server Spans of `a`, the unspecified root of `b` and the consumer
    // root of `c`; not the client Span of `a` nor the internal one of `b`.
    let entry = overview(&source(RedScope::Entry, "[]", None)).await;
    assert_eq!(entry.total.requests, 4);
    assert_eq!(entry.total.errors, Some(2));

    let all = overview(&source(RedScope::All, "[]", None)).await;
    assert_eq!(all.total.requests, 6);
    assert_eq!(all.total.errors, Some(2));
}

#[tokio::test]
async fn the_figures_are_laid_out_in_the_bars_of_the_plan() {
    let overview = overview(&source(RedScope::Entry, "[]", None)).await;
    let buckets = overview.buckets.expect("time is not masked");

    let counted = buckets
        .iter()
        .map(|bucket| {
            (
                bucket.start_time,
                bucket.figures.requests,
                bucket.figures.errors,
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        counted,
        vec![
            (ORIGIN, 2, Some(1)),
            (ORIGIN + 60 * SECOND, 1, Some(0)),
            (ORIGIN + 120 * SECOND, 1, Some(1)),
        ]
    );

    // One request in a bar is its own percentile.
    assert_eq!(buckets[1].figures.p99_ns, Some(1e9));

    let p50 = overview.total.p50_ns.expect("median");
    let p99 = overview.total.p99_ns.expect("99th percentile");
    assert!((2e6..=1e9).contains(&p50), "median {p50}");
    assert!(p50 <= p99);
}

#[tokio::test]
async fn durations_fall_in_the_bar_they_begin_with_an_edge_on_its_start() {
    let latency = overview(&source(RedScope::Entry, "[]", None))
        .await
        .latency
        .into_iter()
        .map(|bar| (bar.from_ns, bar.to_ns, bar.requests, bar.errors))
        .collect::<Vec<_>>();

    assert_eq!(
        latency,
        vec![
            // Two milliseconds exactly.
            (2_000_000, Some(2_500_000), 1, Some(1)),
            (12_500_000, Some(16_000_000), 1, Some(1)),
            (100_000_000, Some(125_000_000), 1, Some(0)),
            // A second exactly.
            (1_000_000_000, Some(1_250_000_000), 1, Some(0)),
        ]
    );
}

#[tokio::test]
async fn a_filter_on_a_field_of_a_span_holds_for_the_span_itself() {
    let checkout = r#"[{"field":"service.name","op":"=","value":"checkout"}]"#;
    assert_eq!(requests(RedScope::Entry, checkout, None).await, 2);
    assert_eq!(requests(RedScope::All, checkout, None).await, 4);

    let failed = r#"[{"field":"http.status_code","op":">=","value":"500"}]"#;
    assert_eq!(requests(RedScope::Entry, failed, None).await, 1);

    let named = r#"[{"field":"name","op":"contains","value":"cart"}]"#;
    assert_eq!(requests(RedScope::Entry, named, None).await, 2);
}

#[tokio::test]
async fn leaving_a_value_out_keeps_the_spans_without_the_field() {
    let elsewhere = r#"[{"field":"service.name","op":"!=","value":"checkout"}]"#;
    assert_eq!(requests(RedScope::All, elsewhere, None).await, 2);

    // All but `a3`, which failed with a 500: `a2`, `b2` and `c1` have no status code.
    let not_failed = r#"[{"field":"http.status_code","op":"!=","value":"500"}]"#;
    assert_eq!(requests(RedScope::All, not_failed, None).await, 5);

    // A condition after the one that leaves a value out still holds beside it: `a3` alone.
    let elsewhere_failed = r#"[{"field":"service.name","op":"!=","value":"checkout"},
                               {"field":"http.status_code","op":"=","value":"500"}]"#;
    assert_eq!(requests(RedScope::All, elsewhere_failed, None).await, 1);
}

#[tokio::test]
async fn trace_fields_are_read_of_the_span_or_of_its_trace() {
    // The Span's own duration: `a1` and `b1` took 100 ms or more.
    let slow = r#"[{"field":"duration_ns","op":">=","value":"100000000"}]"#;
    assert_eq!(requests(RedScope::Entry, slow, None).await, 2);

    // The entry Spans of the traces with a failure: `a1`, `a3` and `c1`.
    let failing = r#"[{"field":"error_count","op":">","value":"0"}]"#;
    assert_eq!(requests(RedScope::Entry, failing, None).await, 3);

    // Every Span of the traces of two Spans or more: `a` and `b`.
    let long = r#"[{"field":"span_count","op":">=","value":"2"}]"#;
    assert_eq!(requests(RedScope::All, long, None).await, 5);

    let one = r#"[{"field":"trace_id","op":"=","value":"b"}]"#;
    assert_eq!(requests(RedScope::All, one, None).await, 2);
}

/// How many Spans of `scope` pass a condition tree.
async fn requests_where(scope: RedScope, tree: Value) -> u64 {
    overview(&source_where(scope, "[]", None, Some(tree)))
        .await
        .total
        .requests
}

fn service(value: &str) -> Value {
    json!({"field": "service.name", "op": "=", "value": value})
}

#[tokio::test]
async fn either_condition_holds_for_the_span_itself() {
    let either = json!({"or": [service("inventory"), service("mailer")]});
    assert_eq!(requests_where(RedScope::All, either).await, 2);

    // A Span without a status code answers the other part: `b2` is `render`.
    let answered_or_rendered = json!({"or": [
        {"field": "http.status_code", "op": "=", "value": "200"},
        {"field": "name", "op": "=", "value": "render"}
    ]});
    assert_eq!(requests_where(RedScope::All, answered_or_rendered).await, 3);

    // A Span without the field keeps a value left out: `a3` and `c1` are not of
    // `checkout`, `a1` and `b1` answered 200.
    let elsewhere_or_ok = json!({"or": [
        {"field": "service.name", "op": "!=", "value": "checkout"},
        {"field": "http.status_code", "op": "=", "value": "200"}
    ]});
    assert_eq!(requests_where(RedScope::All, elsewhere_or_ok).await, 4);

    // Free text takes part as a condition of its own.
    let text_or_mailer = json!({"or": [{"text": "STOCK"}, service("mailer")]});
    assert_eq!(requests_where(RedScope::Entry, text_or_mailer).await, 2);
}

#[tokio::test]
async fn a_tree_groups_what_holds_together() {
    // `(inventory OR mailer) AND status_code = ERROR`: both failed.
    let bracketed = json!({"and": [
        {"or": [service("inventory"), service("mailer")]},
        {"field": "status_code", "op": "=", "value": "ERROR"}
    ]});
    assert_eq!(requests_where(RedScope::All, bracketed).await, 2);

    // `inventory OR mailer AND status_code = OK`: AND first, and `c1` failed.
    let and_first = json!({"or": [
        service("inventory"),
        {"and": [service("mailer"), {"field": "status_code", "op": "=", "value": "OK"}]}
    ]});
    assert_eq!(requests_where(RedScope::All, and_first).await, 1);

    // A count of the trace a Span belongs to beside a field of the Span: `a` has three.
    let long_or_mailer = json!({"or": [
        {"field": "span_count", "op": ">=", "value": "3"},
        service("mailer")
    ]});
    assert_eq!(requests_where(RedScope::All, long_or_mailer).await, 4);
}

#[tokio::test]
async fn free_text_is_looked_for_where_the_list_looks_for_it() {
    assert_eq!(requests(RedScope::Entry, "[]", Some("INVENTORY")).await, 1);
    assert_eq!(requests(RedScope::Entry, "[]", Some("nowhere")).await, 0);
}

#[tokio::test]
async fn nothing_matched_is_no_request_and_nothing_to_lay_out() {
    let none = r#"[{"field":"service.name","op":"=","value":"nobody"}]"#;
    let overview = overview(&source(RedScope::Entry, none, None)).await;

    assert_eq!(
        overview,
        Overview {
            total: Figures {
                requests: 0,
                errors: Some(0),
                ..Figures::default()
            },
            buckets: Some(Vec::new()),
            latency: Vec::new(),
        }
    );
}

#[tokio::test]
async fn a_breakdown_lists_the_commonest_values_first_each_bar_by_bar() {
    let session = session();
    let source = source(RedScope::Entry, "[]", None);

    let breakdown = reading::breakdown(&source, &grouping("service.name"), &plan(), |statement| {
        execute(session.clone(), statement)
    })
    .await
    .expect("breakdown")
    .expect("not masked");

    assert!(!breakdown.more);

    let groups = breakdown
        .groups
        .iter()
        .map(|group| {
            (
                group.value.as_deref(),
                group.figures.requests,
                group.figures.errors,
                group
                    .series
                    .iter()
                    .map(|point| (point.start_time, point.requests))
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        groups,
        vec![
            (
                Some("checkout"),
                2,
                Some(0),
                vec![(ORIGIN, 1), (ORIGIN + 60 * SECOND, 1)]
            ),
            (Some("inventory"), 1, Some(1), vec![(ORIGIN, 1)]),
            (Some("mailer"), 1, Some(1), vec![(ORIGIN + 120 * SECOND, 1)]),
        ]
    );

    assert_eq!(breakdown.groups[2].series[0].p95_ns, Some(2e6));
}

#[tokio::test]
async fn requests_without_a_value_are_a_group_of_their_own() {
    let session = session();
    let source = source(RedScope::Entry, "[]", None);

    let breakdown = reading::breakdown(
        &source,
        &grouping("http.status_code"),
        &plan(),
        |statement| execute(session.clone(), statement),
    )
    .await
    .expect("breakdown")
    .expect("not masked");

    let groups = breakdown
        .groups
        .iter()
        .map(|group| {
            (
                group.value.as_deref(),
                group.figures.requests,
                group.series.len(),
            )
        })
        .collect::<Vec<_>>();

    // A number is named as text; the consumer root of `c` has no status code.
    assert_eq!(
        groups,
        vec![(Some("200"), 2, 2), (Some("500"), 1, 1), (None, 1, 1)]
    );
}

/// A client Span without a parent is a call made outside any request; a server
/// Span is an entry wherever it stands. The same holds where the stream keeps
/// the kind as text, the numbers or the names.
#[tokio::test]
async fn a_call_made_outside_any_request_is_no_entry() {
    for text in [false, true] {
        let schema = Arc::new(ArrowSchema::new(vec![
            Field::new("parent_span_id", DataType::Utf8, true),
            Field::new(
                "kind",
                if text {
                    DataType::Utf8
                } else {
                    DataType::Int64
                },
                true,
            ),
            Field::new("duration_ns", DataType::Int64, false),
        ]));

        let kinds: ArrayRef = if text {
            Arc::new(StringArray::from(vec!["3", "0", "SERVER", "1"]))
        } else {
            Arc::new(Int64Array::from(vec![3, 0, 2, 1]))
        };

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(StringArray::from(vec![
                    None,
                    Some(""),
                    Some("p"),
                    Some("p"),
                ])),
                kinds,
                Arc::new(Int64Array::from(vec![10, 20, 30, 40])),
            ],
        )
        .expect("spans");

        let session = SessionContext::new();

        session
            .register_table(
                "spans",
                Arc::new(MemTable::try_new(schema, vec![vec![batch]]).expect("table")),
            )
            .expect("register");

        let mut definition = definition();

        definition.schema.fields.retain(|field| {
            matches!(
                field.name.as_str(),
                "parent_span_id" | "kind" | "duration_ns"
            )
        });

        for field in &mut definition.schema.fields {
            if text && field.name == "kind" {
                field.data_type = FieldType::Utf8;
            }
        }

        let source = Source::new(
            &definition,
            RedScope::Entry,
            &filter::TraceCondition::of(&[], None, None),
        )
        .expect("source");

        let totals = execute(session, statements::totals(&source))
            .await
            .expect("totals");

        // The root of no stated kind, and the server Span.
        assert_eq!(totals.rows[0][0], json!(2), "kind kept as text: {text}");
    }
}

#[test]
fn a_moment_or_a_document_does_not_group_requests() {
    let field = |data_type| FieldDef {
        name: "x".into(),
        data_type,
        nullable: true,
        index_type: None,
        indexed: false,
        encrypted: false,
        exact: false,
    };

    assert!(Grouping::new(&field(FieldType::Timestamp)).is_none());
    assert!(Grouping::new(&field(FieldType::Json)).is_none());
    assert!(Grouping::new(&field(FieldType::Bool)).is_some());
}

#[test]
fn statements_name_their_columns_apart_from_every_field() {
    let source = source(
        RedScope::Entry,
        r#"[{"field":"span_count","op":">=","value":"2"}]"#,
        None,
    );

    let totals = statements::totals(&source);
    assert!(totals.contains(
        "(\"kind\" IN (2, 5) OR ((\"parent_span_id\" IS NULL OR \"parent_span_id\" = '') AND \
         (\"kind\" IS NULL OR NOT \"kind\" IN (3, 4))))"
    ));
    assert!(totals.contains(
        "\"trace_id\" IN (SELECT \"trace_id\" FROM \"spans\" GROUP BY \"trace_id\" HAVING COUNT(*) >= 2)"
    ));

    for column in [
        "__red_requests",
        "__red_errors",
        "__red_p50",
        "__red_p95",
        "__red_p99",
    ] {
        assert!(totals.contains(&format!("AS {column}")), "{column}");
    }

    assert!(
        statements::series(&source, &plan())
            .contains("(CAST(_timestamp AS BIGINT) - 600000000) / 60000000 AS __red_bucket")
    );
}

#[test]
fn the_edges_of_the_duration_bars_are_the_r10_series_in_each_power_of_ten() {
    let edges = statements::latency_edges();

    assert_eq!(&edges[..6], &[100, 125, 160, 200, 250, 315]);
    assert_eq!(&edges[10..12], &[1_000, 1_250]);
    assert_eq!(edges.len(), 101);
    assert_eq!(edges.last(), Some(&1_000_000_000_000));
    assert!(edges.windows(2).all(|pair| pair[0] < pair[1]));
}

#[tokio::test]
async fn durations_past_the_edges_fall_in_the_first_and_the_last_bar() {
    let session = SessionContext::new();
    let schema = Arc::new(ArrowSchema::new(vec![Field::new(
        "duration_ns",
        DataType::Int64,
        false,
    )]));

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![Arc::new(Int64Array::from(vec![
            0,
            99,
            124,
            125,
            999_999_999_999,
            1_000_000_000_000,
            i64::MAX,
        ]))],
    )
    .expect("durations");

    session
        .register_table(
            "spans",
            Arc::new(MemTable::try_new(schema, vec![vec![batch]]).expect("table")),
        )
        .expect("register");

    let mut definition = definition();
    definition
        .schema
        .fields
        .retain(|field| field.name == "duration_ns");

    let source = Source::new(
        &definition,
        RedScope::All,
        &filter::TraceCondition::of(&[], None, None),
    )
    .expect("source");
    let result = execute(session, statements::latency(&source))
        .await
        .expect("latency");

    let mut bars = result
        .rows
        .iter()
        .map(|row| {
            (
                row[0].as_u64().expect("bar"),
                row[1].as_u64().expect("count"),
            )
        })
        .collect::<Vec<_>>();

    bars.sort_unstable();

    assert_eq!(bars, vec![(0, 3), (1, 1), (99, 1), (100, 2)]);
}

#[tokio::test]
async fn figures_that_come_back_masked_are_not_told() {
    // Failures and durations as text: the fields they are read from are masked.
    let masked = result(
        &[
            "__red_bucket",
            "__red_bar",
            "__red_requests",
            "__red_errors",
            "__red_p50",
        ],
        vec![vec![
            json!(0),
            json!(0),
            json!(3),
            json!("***"),
            json!("***"),
        ]],
    );

    let overview = reading::overview(&source(RedScope::Entry, "[]", None), &plan(), |_| {
        let masked = masked.clone();

        async move { Ok::<_, Error>(masked) }
    })
    .await
    .expect("overview");

    assert_eq!(overview.total.requests, 3);
    assert_eq!(overview.total.errors, None);
    assert_eq!(overview.total.p50_ns, None);
    assert_eq!(overview.latency[0].errors, None);

    // The bars as text: time is masked.
    let overview = reading::overview(
        &source(RedScope::Entry, "[]", None),
        &plan(),
        |statement: String| {
            let answer = if statement.contains("__red_bucket") {
                result(
                    &["__red_bucket", "__red_requests"],
                    vec![vec![json!("***"), json!(3)]],
                )
            } else {
                result(
                    &["__red_requests", "__red_bar"],
                    vec![vec![json!(3), json!(0)]],
                )
            };

            async move { Ok::<_, Error>(answer) }
        },
    )
    .await
    .expect("overview");

    assert_eq!(overview.buckets, None);
}

#[tokio::test]
async fn a_masked_field_has_no_groups_to_list() {
    let ranked = result(
        &["__red_value", "__red_present", "__red_requests"],
        vec![vec![json!("***"), json!("***"), json!(3)]],
    );

    let breakdown = reading::breakdown(
        &source(RedScope::Entry, "[]", None),
        &grouping("service.name"),
        &plan(),
        |_| {
            let ranked = ranked.clone();

            async move { Ok::<_, Error>(ranked) }
        },
    )
    .await
    .expect("breakdown");

    assert_eq!(breakdown, None);
}

#[test]
fn a_read_that_cannot_be_had_says_why() {
    assert_eq!(settled(Ok(Some(1))).expect("answer"), Ok(1));
    assert_eq!(
        settled::<()>(Ok(None)).expect("reason"),
        Err(RedUnavailable::TimeBudget)
    );
    assert_eq!(
        settled::<()>(Err(Error::resource_exhausted("busy"))).expect("reason"),
        Err(RedUnavailable::Busy)
    );

    let over = ensure_within_scan_limit(11, 10).expect_err("over the limit");
    assert_eq!(
        settled::<()>(Err(over)).expect("reason"),
        Err(RedUnavailable::ScanLimit)
    );

    assert!(settled::<()>(Err(Error::invalid("unsupported trace filter field: x"))).is_err());
}

/// The Grafana plugin and other clients read the RED figures by these names.
#[test]
fn red_figures_keep_the_names_clients_read() {
    let figures = || Figures {
        requests: 4,
        errors: Some(1),
        p50_ns: Some(10.0),
        p95_ns: Some(20.0),
        p99_ns: Some(30.0),
    };
    let response = super::TraceRedResponse {
        bucket_time: 60_000_000,
        origin_time: 0,
        scope: RedScope::Entry,
        total: figures(),
        buckets: vec![reading::Bucket {
            start_time: 0,
            figures: figures(),
        }],
        latency: Vec::new(),
        unavailable: None,
    };

    assert_eq!(
        serde_json::to_value(response).unwrap(),
        json!({
            "bucketTime": 60_000_000,
            "originTime": 0,
            "scope": "entry",
            "total": {"requests": 4, "errors": 1, "p50Ns": 10.0, "p95Ns": 20.0, "p99Ns": 30.0},
            "buckets": [{
                "startTime": 0,
                "requests": 4, "errors": 1, "p50Ns": 10.0, "p95Ns": 20.0, "p99Ns": 30.0
            }],
            "latency": []
        })
    );
}
