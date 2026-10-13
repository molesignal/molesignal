// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The statements run against rows the way the engine plans them, and read back.

use std::sync::Arc;

use arrow::{
    array::{
        ArrayRef, Float64Array, Int64Array, RecordBatch, StringArray, TimestampMicrosecondArray,
    },
    datatypes::{DataType, Field, Schema, TimeUnit},
};
use datafusion::{datasource::MemTable, prelude::SessionContext};
use sqlparser::{dialect::GenericDialect, parser::Parser};

use super::*;
use crate::infra::query::{analyzer::session_context, arrow_json::batches_to_json};

/// Eight rows, the last of them outside the conditions of the scope.
fn rows() -> RecordBatch {
    let times = [1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000, 9_000_000];
    let status = [
        Some(200),
        Some(200),
        Some(200),
        Some(404),
        Some(500),
        None,
        Some(503),
        Some(200),
    ];
    let service = [
        Some("api"),
        Some("api"),
        Some("web"),
        Some(""),
        None,
        Some("api"),
        Some("web"),
        Some("db"),
    ];
    let latency = [
        Some(0.5),
        Some(1.5),
        Some(2.5),
        None,
        Some(0.25),
        Some(8.0),
        Some(1.0),
        Some(99.0),
    ];

    let schema = Schema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new("status", DataType::Int64, true),
        Field::new("service.name", DataType::Utf8, true),
        Field::new("latency", DataType::Float64, true),
    ]);

    let arrays: Vec<ArrayRef> = vec![
        Arc::new(TimestampMicrosecondArray::from(times.to_vec()).with_timezone("UTC")),
        Arc::new(Int64Array::from(status.to_vec())),
        Arc::new(StringArray::from(service.to_vec())),
        Arc::new(Float64Array::from(latency.to_vec())),
    ];

    RecordBatch::try_new(Arc::new(schema), arrays).expect("record batch")
}

fn scope() -> Scope {
    Scope {
        stream: "app_logs".into(),
        stream_type: StreamType::LOGS,
        from: 0,
        to: 8_000,
        clauses: vec![
            "CAST(_timestamp AS BIGINT) >= 0".into(),
            "CAST(_timestamp AS BIGINT) < 8000".into(),
        ],
    }
}

fn session() -> SessionContext {
    let session = session_context(0);
    let batch = rows();
    let table = MemTable::try_new(batch.schema(), vec![vec![batch]]).expect("mem table");

    session
        .register_table("app_logs", Arc::new(table))
        .expect("register table");

    session
}

async fn run_on(session: &SessionContext, statement: String) -> Result<QueryResult> {
    let batches = session
        .sql(&statement)
        .await
        .expect("the statement should plan")
        .collect()
        .await
        .expect("the statement should execute");
    let (columns, rows) = batches_to_json(&batches);

    Ok(QueryResult {
        columns,
        rows,
        scanned_rows: 0,
        took_ms: 0,
        federation: None,
    })
}

async fn analysed(name: &str, field_type: FieldType) -> FieldStatsResponse {
    let session = session();
    let field = Analysed::new(name, field_type).expect("field");

    read(&scope(), &field, |statement| run_on(&session, statement))
        .await
        .expect("analysis")
}

fn top(value: &str, count: u64) -> TopValue {
    TopValue {
        value: value.into(),
        count,
    }
}

#[tokio::test]
async fn a_whole_number_is_counted_ranked_and_measured() {
    let stats = analysed("status", FieldType::Int64).await;

    // The row outside the scope and the row without a status are not values.
    assert_eq!((stats.total, stats.present), (7, 6));
    assert_eq!((stats.distinct, stats.distinct_exact), (4, true));
    assert_eq!(
        stats.top,
        [top("200", 3), top("404", 1), top("500", 1), top("503", 1)]
    );
    assert_eq!(
        stats.range,
        Some(ValueRange {
            min: "200".into(),
            max: "503".into(),
        })
    );

    let spread = stats.spread.expect("a number has a spread");
    let avg = (200.0 * 3.0 + 404.0 + 500.0 + 503.0) / 6.0;
    assert!((spread.avg.expect("avg") - avg).abs() < 1e-9);
    assert!(spread.stddev_samp.expect("sample") > spread.stddev_pop.expect("population"));

    let (q1, median, q3) = (
        spread.q1.expect("q1"),
        spread.median.expect("median"),
        spread.q3.expect("q3"),
    );
    assert!(200.0 <= q1 && q1 <= median && median <= q3 && q3 <= 503.0);

    // Half the values are 200 and the rest lie far above: the tail is to the right.
    assert!(spread.skewness.expect("skewness") > 0.0);
    assert!(spread.kurtosis.expect("kurtosis").is_finite());

    let bars = stats.bars.expect("a number has bars");
    assert_eq!((bars.origin, bars.width), (200.0, 20.0));
    assert_eq!(bars.counts.len(), 16);
    assert_eq!(bars.counts.iter().sum::<u64>(), 6);
    assert_eq!(
        (bars.counts[0], bars.counts[10], bars.counts[15]),
        (3, 1, 2)
    );

    // A code is followed over time like any value that repeats: the three 200s
    // lie in the first three milliseconds of the eight.
    let trend = stats.trend.expect("a value that repeats has a trend");
    assert_eq!((trend.origin_micros, trend.bucket_micros), (0, 1_000_000));
    assert_eq!(trend.series.len(), 4);
    assert_eq!(trend.series[0].value, "200");
    assert_eq!(trend.series[0].counts, [3]);
}

#[tokio::test]
async fn a_fraction_is_counted_into_bars_as_fine_as_it_is() {
    let stats = analysed("latency", FieldType::Float64).await;

    assert_eq!((stats.total, stats.present), (7, 6));

    let bars = stats.bars.expect("bars");
    assert_eq!(bars.origin, 0.0);
    assert_eq!(bars.width, 0.5);
    assert_eq!(bars.counts.iter().sum::<u64>(), 6);
    // 0.25 lies in the first bar and 8.0 opens the last.
    assert_eq!(bars.counts.first(), Some(&1));
    assert_eq!(bars.counts.last(), Some(&1));
}

#[tokio::test]
async fn text_is_counted_and_ranked_and_an_empty_string_is_no_value() {
    let stats = analysed("service.name", FieldType::Utf8).await;

    assert_eq!((stats.total, stats.present), (7, 5));
    assert_eq!((stats.distinct, stats.distinct_exact), (2, true));
    assert_eq!(stats.top, [top("api", 3), top("web", 2)]);
    assert_eq!(stats.range, None);
    assert_eq!(stats.spread, None);
    assert_eq!(stats.bars, None);
}

#[tokio::test]
async fn the_commonest_values_are_followed_over_time() {
    let session = session();
    let field = Analysed::new("service.name", FieldType::Utf8).expect("field");

    // The same seven rows, over a range short enough for a stretch to be a millisecond.
    let narrow = Scope {
        from: 1_000,
        to: 7_001,
        ..scope()
    };

    // A second is the shortest stretch there is: seven milliseconds are one of it.
    let stats = read(&narrow, &field, |statement| run_on(&session, statement))
        .await
        .expect("analysis");
    let trend = stats.trend.expect("trend");

    assert_eq!((trend.origin_micros, trend.bucket_micros), (0, 1_000_000));
    assert_eq!(
        trend.series,
        [
            TrendSeries {
                value: "api".into(),
                counts: vec![3],
            },
            TrendSeries {
                value: "web".into(),
                counts: vec![2],
            },
        ]
    );
}

#[tokio::test]
async fn a_trend_counts_each_value_in_the_stretch_it_fell_in() {
    let session = session();
    let field = Analysed::new("service.name", FieldType::Utf8).expect("field");

    // The row nine seconds in is within reach: ten seconds are ten stretches.
    let wide = Scope {
        from: 0,
        to: 10_000_000,
        clauses: vec![
            "CAST(_timestamp AS BIGINT) >= 0".into(),
            "CAST(_timestamp AS BIGINT) < 10000000".into(),
        ],
        ..scope()
    };

    let stats = read(&wide, &field, |statement| run_on(&session, statement))
        .await
        .expect("analysis");
    let trend = stats.trend.expect("trend");

    assert_eq!(trend.bucket_micros, 1_000_000);
    assert_eq!(trend.series.len(), 3);
    assert_eq!(trend.series[0].value, "api");
    assert_eq!(trend.series[0].counts, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    // "db" was written once, in the last second.
    assert_eq!(trend.series[2].value, "db");
    assert_eq!(trend.series[2].counts, [0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
}

#[tokio::test]
async fn values_met_once_each_have_no_trend() {
    let stats = analysed("latency", FieldType::Float64).await;

    assert_eq!(stats.top.first().map(|top| top.count), Some(1));
    assert_eq!(stats.trend, None);
}

#[tokio::test]
async fn a_moment_has_an_earliest_and_a_latest_and_no_average() {
    let stats = analysed("_timestamp", FieldType::Timestamp).await;

    assert_eq!((stats.total, stats.present), (7, 7));
    assert_eq!(
        stats.range,
        Some(ValueRange {
            min: "1000".into(),
            max: "7000".into(),
        })
    );
    assert_eq!(stats.spread, None);
    assert_eq!(stats.bars, None);
}

#[tokio::test]
async fn a_field_without_values_is_only_counted() {
    let session = session();
    let field = Analysed::new("status", FieldType::Int64).expect("field");

    let nothing = Scope {
        clauses: vec!["\"status\" IS NULL".into()],
        ..scope()
    };

    let stats = read(&nothing, &field, |statement| run_on(&session, statement))
        .await
        .expect("analysis");

    assert_eq!((stats.total, stats.present, stats.distinct), (1, 0, 0));
    assert!(stats.top.is_empty());
    assert_eq!(stats.bars, None);
}

#[test]
fn a_json_field_is_not_analysed() {
    assert!(Analysed::new("payload", FieldType::Json).is_err());
}

/// The masking of a result is worked out from the statement as the generic
/// dialect parses it: a statement it cannot parse fails before it is run.
#[test]
fn every_statement_parses_in_the_dialect_masking_reads() {
    let scope = scope();
    let number = Analysed::new("status", FieldType::Int64).expect("field");
    let text = Analysed::new("service.name", FieldType::Utf8).expect("field");
    let plan = BinPlan::new(-2.5, 1.0e21, false);

    let mut written = vec![
        statements::summary(&scope, &number),
        statements::summary(&scope, &text),
        statements::top_values(&scope, &text, 11),
    ];
    written.extend(plan.map(|plan| statements::bars(&scope, &number, &plan, -0.125)));
    written.extend(
        TrendPlan::new(scope.from, scope.to)
            .map(|plan| statements::trend(&scope, &text, &plan, &["it's", "api"])),
    );

    for statement in written {
        Parser::parse_sql(&GenericDialect, &statement)
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
}
