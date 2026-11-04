// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{AvailabilityQuery, Summary, bucket_start, summaries};
use crate::{domain::query::QueryResult, shared::ids::Id};

const HOUR: i64 = 60 * 60 * 1_000_000;

fn query(monitor: Option<&str>, durations: bool) -> AvailabilityQuery {
    AvailabilityQuery::new(
        0,
        24 * HOUR,
        None,
        monitor.map(Id::from_string),
        None,
        durations,
    )
    .expect("a valid query")
}

#[test]
fn a_range_is_cut_into_steps_a_chart_can_show() {
    // A day in quarter hours, a week in three hours: as many steps as can be read.
    assert_eq!(query(None, true).step, HOUR / 4);

    let week = AvailabilityQuery::new(0, 7 * 24 * HOUR, None, None, None, true).expect("week");
    assert_eq!(week.step, 3 * HOUR);

    // A step asked for stands, unless it would cut the range too fine.
    let fine = AvailabilityQuery::new(0, 6 * HOUR, Some(60), None, None, true).expect("fine");
    assert_eq!(fine.step, 60 * 1_000_000);

    let too_fine =
        AvailabilityQuery::new(0, 30 * 24 * HOUR, Some(60), None, None, true).expect("too fine");
    assert!(30 * 24 * HOUR / too_fine.step <= 500);
}

#[test]
fn only_a_plain_id_and_a_forward_range_are_asked_about() {
    assert!(AvailabilityQuery::new(10, 10, None, None, None, true).is_err());

    let quoted = Some(Id::from_string("x' OR '1'='1"));
    assert!(AvailabilityQuery::new(0, HOUR, None, quoted, None, true).is_err());
}

#[test]
fn the_statements_read_the_range_of_one_check() {
    let one = query(Some("checkout"), true);

    assert!(one.total_sql().contains("FROM _synthetic_results"));
    assert!(
        one.total_sql()
            .contains(&format!("CAST(_timestamp AS BIGINT) < {}", 24 * HOUR))
    );
    assert!(one.checks_sql().contains("check_id = 'checkout'"));
    assert!(one.locations_sql().contains("GROUP BY location_id"));
    assert!(one.buckets_sql().contains(&format!("/ {}", HOUR / 4)));
    assert!(one.location_buckets_sql().is_some());
    assert!(one.total_sql().contains("approx_percentile_cont(0.95)"));

    // Without a duration yet the stream has no such column to read.
    let none = query(None, false);
    assert!(!none.total_sql().contains("duration_ms"));
    assert!(none.location_buckets_sql().is_none());
}

#[test]
fn a_row_reads_as_how_its_runs_went() {
    let result = QueryResult {
        columns: [
            "location_id",
            "runs",
            "available",
            "failing",
            "p50_ms",
            "p95_ms",
            "max_ms",
        ]
        .map(String::from)
        .to_vec(),
        rows: vec![
            vec![
                json!("fra"),
                json!(10),
                json!(8),
                json!(1),
                json!(120.5),
                json!(300.0),
                json!(410.0),
            ],
            vec![
                json!("sin"),
                json!("3"),
                json!(0),
                json!(0),
                json!(null),
                json!(null),
                json!(null),
            ],
        ],
        scanned_rows: 13,
        took_ms: 1,
        federation: None,
    };

    let rows = summaries(&result, &["location_id"]);

    assert_eq!(rows[0].0, vec![json!("fra")]);
    assert_eq!(
        rows[0].1,
        Summary {
            runs: 10,
            available: 8,
            failing: 1,
            undecided: 1,
            availability: Some(8.0 / 9.0),
            p50_ms: Some(120.5),
            p95_ms: Some(300.0),
            max_ms: Some(410.0),
        }
    );

    // Runs that were never judged say nothing of availability.
    assert_eq!(rows[1].1.undecided, 3);
    assert_eq!(rows[1].1.availability, None);
}

#[test]
fn a_step_starts_where_its_index_says() {
    let day = query(None, true);

    assert_eq!(bucket_start(&day, &json!(3)), 3 * HOUR / 4);
    assert_eq!(bucket_start(&day, &json!("4")), HOUR);
}

mod engine {
    //! The statements as the query engine runs them, over a table of runs.

    use std::sync::Arc;

    use datafusion::{
        arrow::{
            array::{ArrayRef, Float64Array, RecordBatch, StringArray, TimestampMicrosecondArray},
            datatypes::{DataType, Field, Schema, TimeUnit},
        },
        prelude::SessionContext,
    };

    use super::{
        super::{AvailabilityQuery, summaries, text},
        HOUR,
    };
    use crate::{
        domain::query::QueryResult, infra::query::arrow_json::batches_to_json, shared::ids::Id,
    };

    /// Runs as `(started, check, location, outcome, duration)`.
    type Run = (i64, &'static str, &'static str, &'static str, Option<f64>);

    const RUNS: [Run; 6] = [
        (HOUR, "checkout", "fra", "healthy", Some(100.0)),
        (2 * HOUR, "checkout", "fra", "failing", Some(300.0)),
        (2 * HOUR + 1, "checkout", "sin", "degraded", Some(200.0)),
        (3 * HOUR, "checkout", "sin", "skipped", None),
        (4 * HOUR, "search", "fra", "healthy", Some(50.0)),
        // After the range ends.
        (25 * HOUR, "checkout", "fra", "failing", Some(999.0)),
    ];

    fn table(durations: bool) -> SessionContext {
        let mut fields = vec![
            Field::new(
                "_timestamp",
                DataType::Timestamp(TimeUnit::Microsecond, None),
                false,
            ),
            Field::new("check_id", DataType::Utf8, false),
            Field::new("location_id", DataType::Utf8, false),
            Field::new("outcome", DataType::Utf8, false),
        ];

        let mut columns: Vec<ArrayRef> = vec![
            Arc::new(TimestampMicrosecondArray::from_iter_values(
                RUNS.map(|run| run.0),
            )),
            Arc::new(StringArray::from_iter_values(RUNS.map(|run| run.1))),
            Arc::new(StringArray::from_iter_values(RUNS.map(|run| run.2))),
            Arc::new(StringArray::from_iter_values(RUNS.map(|run| run.3))),
        ];

        if durations {
            fields.push(Field::new("duration_ms", DataType::Float64, true));
            columns.push(Arc::new(Float64Array::from_iter(RUNS.map(|run| run.4))));
        }

        let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).expect("batch");
        let context = SessionContext::new();

        context
            .register_batch("_synthetic_results", batch)
            .expect("register");

        context
    }

    async fn run(context: &SessionContext, statement: String) -> QueryResult {
        let batches = context
            .sql(&statement)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"))
            .collect()
            .await
            .expect("collect");

        let (columns, rows) = batches_to_json(&batches);

        QueryResult {
            columns,
            rows,
            scanned_rows: 0,
            took_ms: 0,
            federation: None,
        }
    }

    #[tokio::test]
    async fn the_engine_counts_the_runs_of_the_range() {
        let context = table(true);
        let day = AvailabilityQuery::new(0, 24 * HOUR, None, None, None, true).expect("day");

        let total = summaries(&run(&context, day.total_sql()).await, &[]);
        let total = &total[0].1;

        assert_eq!(
            (total.runs, total.available, total.failing, total.undecided),
            (5, 3, 1, 1)
        );
        assert_eq!(total.availability, Some(0.75));
        assert_eq!(total.max_ms, Some(300.0));
        assert!(
            total
                .p50_ms
                .is_some_and(|p50| (50.0..=300.0).contains(&p50))
        );

        let mut checks = summaries(&run(&context, day.checks_sql()).await, &["check_id"]);
        checks.sort_by_key(|(keys, _)| text(&keys[0]));

        assert_eq!(text(&checks[0].0[0]), "checkout");
        assert_eq!(checks[0].1.availability, Some(2.0 / 3.0));
        assert_eq!(checks[1].1.availability, Some(1.0));

        // Four runs in the range, in four quarter hours.
        let buckets = summaries(&run(&context, day.buckets_sql()).await, &["bucket"]);
        assert_eq!(buckets.len(), 4);
    }

    #[tokio::test]
    async fn the_engine_draws_each_place_of_one_check() {
        let context = table(true);
        let checkout = Some(Id::from_string("checkout"));

        let one = AvailabilityQuery::new(0, 24 * HOUR, None, checkout, None, true).expect("one");
        let statement = one.location_buckets_sql().expect("one check");
        let steps = summaries(&run(&context, statement).await, &["location_id", "bucket"]);

        let places = steps
            .iter()
            .map(|(keys, _)| text(&keys[0]))
            .collect::<Vec<_>>();

        assert_eq!(places, ["fra", "fra", "sin", "sin"]);

        let locations = summaries(&run(&context, one.locations_sql()).await, &["location_id"]);
        assert_eq!(locations.len(), 2);
    }

    /// Before any run took time the stream has no duration to read, and the statements still run.
    #[tokio::test]
    async fn the_engine_reads_a_stream_without_durations() {
        let context = table(false);
        let day = AvailabilityQuery::new(0, 24 * HOUR, None, None, None, false).expect("day");

        let total = summaries(&run(&context, day.total_sql()).await, &[]);

        assert_eq!(total[0].1.runs, 5);
        assert_eq!(total[0].1.p95_ms, None);
    }
}
