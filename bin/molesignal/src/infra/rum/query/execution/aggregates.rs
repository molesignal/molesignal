// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The aggregates, run on the engine that answers them in production: that it plans them, that a
//! view or a session counts once at its latest revision, that a measure takes the rows it is
//! asked to, and that a series follows the groups the first statement found.

use arrow::{
    array::{Array, AsArray},
    datatypes::{Float64Type, Int64Type, UInt64Type},
};
use serde_json::{Value, json};

use super::*;
use crate::infra::rum::query::{AggregateSpec, BucketPlan, MeasureSpec, Plan};

fn cell_json(column: &ArrayRef, index: usize) -> Value {
    if column.is_null(index) {
        return Value::Null;
    }

    match column.data_type() {
        DataType::Int64 => json!(column.as_primitive::<Int64Type>().value(index)),
        DataType::UInt64 => json!(column.as_primitive::<UInt64Type>().value(index)),
        DataType::Float64 => json!(column.as_primitive::<Float64Type>().value(index)),
        DataType::Utf8 => json!(column.as_string::<i32>().value(index)),
        DataType::Utf8View => json!(column.as_string_view().value(index)),
        other => panic!("the test cannot read a {other:?} column"),
    }
}

/// The columns and the rows of a statement as the query service hands them over.
pub(super) async fn run(context: &SessionContext, sql: &str) -> (Vec<String>, Vec<Vec<Value>>) {
    let frame = context
        .sql(sql)
        .await
        .unwrap_or_else(|error| panic!("{error}\n{sql}"));

    let columns = frame
        .schema()
        .fields()
        .iter()
        .map(|field| field.name().clone())
        .collect();

    let batches = frame
        .collect()
        .await
        .unwrap_or_else(|error| panic!("{error}\n{sql}"));

    let rows = batches
        .iter()
        .flat_map(|batch| {
            (0..batch.num_rows()).map(move |index| {
                batch
                    .columns()
                    .iter()
                    .map(|column| cell_json(column, index))
                    .collect::<Vec<_>>()
            })
        })
        .collect();

    (columns, rows)
}

fn measure(function: &str, field: Option<&str>, name: Option<&str>) -> MeasureSpec {
    MeasureSpec {
        function: function.into(),
        field: field.map(str::to_string),
        fallback: None,
        filters: Vec::new(),
        name: name.map(str::to_string),
    }
}

fn taking(mut spec: MeasureSpec, field: &str, op: &str, value: &str) -> MeasureSpec {
    spec.filters.push(Filter {
        field: field.into(),
        op: op.into(),
        value: value.into(),
        quoted: false,
    });

    spec
}

fn plan(stream: RumStream, measures: &[MeasureSpec], group_by: &[&str], limit: usize) -> Plan {
    let group_by = group_by
        .iter()
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();

    Plan::new(
        stream,
        &AggregateSpec {
            measures,
            group_by: &group_by,
            bars: None,
            order_by: None,
            descending: None,
            limit: Some(limit),
        },
    )
    .expect("a plan")
}

const LCP: &str = "view.performance.lcp.timestamp";

fn page(
    event: &'static str,
    id: &'static str,
    revision: i64,
    ts: i64,
    group: Option<&'static str>,
    lcp: Option<i64>,
    errors: i64,
) -> Written {
    let mut cells = vec![
        ("view.id", Cell::Text(id)),
        ("view.revision", Cell::Int(revision)),
        ("view.error.count", Cell::Int(errors)),
        ("session.id", Cell::Text("s-1")),
    ];

    if let Some(group) = group {
        cells.push(("view.url_path_group", Cell::Text(group)));
    }

    if let Some(lcp) = lcp {
        cells.push((LCP, Cell::Int(lcp)));
    }

    written(ts, event, cells)
}

fn pages() -> SessionContext {
    context(
        RumStream::Views,
        &[
            // v-1 is reported twice; only what it came to counts.
            page("e1", "v-1", 1, 1_000, Some("/"), Some(1_000_000_000), 0),
            page("e2", "v-1", 2, 1_000, Some("/"), Some(2_000_000_000), 1),
            page("e3", "v-2", 1, 2_000, Some("/"), Some(4_000_000_000), 0),
            page("e4", "v-3", 1, 3_000, Some("/cart"), Some(3_000_000_000), 2),
            page("e5", "v-4", 1, 4_000, Some("/cart"), None, 0),
            page("e6", "v-5", 1, 5_000, None, Some(1_000_000_000), 0),
        ],
    )
}

#[tokio::test]
async fn pages_are_ranked_by_their_views_each_view_counted_once() {
    let context = pages();

    let measures = [
        measure("count", None, Some("views")),
        measure("p75", Some(LCP), Some("lcp")),
        measure("count", Some(LCP), Some("lcp_samples")),
        taking(
            measure("count", None, Some("with_errors")),
            "view.error.count",
            ">",
            "0",
        ),
        taking(
            measure("p75", Some(LCP), Some("clean_lcp")),
            "view.error.count",
            "=",
            "0",
        ),
    ];

    let plan = plan(RumStream::Views, &measures, &["view.url_path_group"], 10);
    let (columns, rows) = run(&context, &plan.first_sql(&scope(RumStream::Views, &[]))).await;
    let answer = plan.answer(&columns, &rows).expect("an answer");

    // Two views each, so the groups come in the order of their names; the one without a group
    // has fewer views and comes last.
    let groups = answer
        .rows
        .iter()
        .map(|row| row.group[0].as_deref())
        .collect::<Vec<_>>();

    assert_eq!(groups, [Some("/"), Some("/cart"), None]);

    let values = |at: usize, name: &str| {
        let index = answer
            .measures
            .iter()
            .position(|measure| measure.name == name)
            .expect(name);

        answer.rows[at].values[index].clone()
    };

    assert_eq!(values(0, "views"), json!(2));
    assert_eq!(values(1, "views"), json!(2));
    assert_eq!(values(2, "views"), json!(1));

    // The first revision of v-1 had no error; the latest has one.
    assert_eq!(values(0, "with_errors"), json!(1));
    assert_eq!(values(1, "with_errors"), json!(1));
    assert_eq!(values(2, "with_errors"), json!(0));

    // A count of a column counts the rows that have it.
    assert_eq!(values(0, "lcp_samples"), json!(2));
    assert_eq!(values(1, "lcp_samples"), json!(1));

    // The percentile is of the latest values: 2 s and 4 s, not 1 s.
    let lcp = values(0, "lcp").as_f64().expect("a number");

    assert!((2_000_000_000.0..=4_000_000_000.0).contains(&lcp), "{lcp}");
    assert_eq!(values(1, "lcp"), json!(3_000_000_000.0));
    assert_eq!(values(2, "lcp"), json!(1_000_000_000.0));

    // Only the views without an error: v-2 of `/`, and none of `/cart` that has a value.
    assert_eq!(values(0, "clean_lcp"), json!(4_000_000_000.0));
    assert_eq!(values(1, "clean_lcp"), Value::Null);

    assert_eq!(answer.measures[1].unit, "ns");
    assert_eq!(answer.measures[0].unit, "count");
}

#[tokio::test]
async fn ordering_by_a_measure_ascending_and_limiting_keeps_the_end_asked_for() {
    let context = pages();

    let measures = [
        measure("count", None, Some("views")),
        measure("max", Some(LCP), Some("slowest")),
    ];

    let group_by = ["view.url_path_group".to_string()];

    let plan = Plan::new(
        RumStream::Views,
        &AggregateSpec {
            measures: &measures,
            group_by: &group_by,
            bars: None,
            order_by: Some("slowest"),
            descending: Some(false),
            limit: Some(2),
        },
    )
    .expect("a plan");

    let (columns, rows) = run(&context, &plan.first_sql(&scope(RumStream::Views, &[]))).await;
    let answer = plan.answer(&columns, &rows).expect("an answer");

    let groups = answer
        .rows
        .iter()
        .map(|row| row.group[0].as_deref())
        .collect::<Vec<_>>();

    // The smallest slowest values: the group without a name (1 s), then `/cart` (3 s).
    assert_eq!(groups, [None, Some("/cart")]);
}

#[tokio::test]
async fn nothing_grouped_is_one_row_over_every_row() {
    let context = pages();

    let measures = [
        measure("count", None, Some("views")),
        measure("sum", Some("view.error.count"), Some("errors")),
        measure("avg", Some("view.error.count"), Some("mean_errors")),
        measure("min", Some(LCP), Some("fastest")),
    ];

    let plan = plan(RumStream::Views, &measures, &[], 10);
    let (columns, rows) = run(&context, &plan.first_sql(&scope(RumStream::Views, &[]))).await;
    let answer = plan.answer(&columns, &rows).expect("an answer");

    assert_eq!(answer.rows.len(), 1);
    assert!(answer.rows[0].group.is_empty());
    // A count is a whole number; a sum is made of shares, so it is a decimal one.
    assert_eq!(
        answer.rows[0].values,
        [json!(5), json!(3.0), json!(0.6), json!(1_000_000_000)]
    );
}

fn session(
    event: &'static str,
    id: &'static str,
    revision: i64,
    user: Option<&'static str>,
    anonymous: Option<&'static str>,
    errors: i64,
) -> Written {
    let mut cells = vec![
        ("session.id", Cell::Text(id)),
        ("session.revision", Cell::Int(revision)),
        ("session.error.count", Cell::Int(errors)),
    ];

    if let Some(user) = user {
        cells.push(("usr.id", Cell::Text(user)));
    }

    if let Some(anonymous) = anonymous {
        cells.push(("usr.anonymous_id", Cell::Text(anonymous)));
    }

    written(1_000, event, cells)
}

#[tokio::test]
async fn users_without_an_id_are_told_apart_by_their_anonymous_id() {
    let context = context(
        RumStream::Sessions,
        &[
            session("e1", "s-1", 1, Some("u-1"), Some("a-9"), 1),
            // s-1 again, as it ended: one session, no error after all.
            session("e2", "s-1", 2, Some("u-1"), Some("a-9"), 0),
            session("e3", "s-2", 1, Some("u-1"), None, 0),
            session("e4", "s-3", 1, None, Some("a-1"), 2),
            session("e5", "s-4", 1, None, Some("a-1"), 0),
            session("e6", "s-5", 1, None, None, 0),
        ],
    );

    let mut users = measure("count_distinct", Some("usr.id"), Some("users"));
    users.fallback = Some("usr.anonymous_id".into());

    let measures = [
        measure("count", None, Some("sessions")),
        measure(
            "count_distinct",
            Some("session.id"),
            Some("distinct_sessions"),
        ),
        users,
        taking(
            measure("count", None, Some("error_free")),
            "session.error.count",
            "=",
            "0",
        ),
    ];

    let plan = plan(RumStream::Sessions, &measures, &[], 10);
    let (columns, rows) = run(&context, &plan.first_sql(&scope(RumStream::Sessions, &[]))).await;
    let answer = plan.answer(&columns, &rows).expect("an answer");

    // Five sessions, two people told apart (u-1, and a-1 by the anonymous id; s-5 has neither),
    // and four sessions free of errors: s-3 is the only one that has one.
    assert_eq!(
        answer.rows[0].values,
        [json!(5), json!(5), json!(2), json!(4)]
    );
}

fn error(event: &'static str, ts: i64, browser: &'static str) -> Written {
    written(
        ts,
        event,
        vec![
            ("browser.name", Cell::Text(browser)),
            ("session.id", Cell::Text("s-1")),
        ],
    )
}

#[tokio::test]
async fn a_series_follows_the_groups_that_rank_first_through_the_bars() {
    let context = context(
        RumStream::Errors,
        &[
            error("e1", 100, "Chrome"),
            error("e2", 200, "Chrome"),
            error("e3", 1_100, "Safari"),
            error("e4", 1_200, "Chrome"),
            error("e5", 2_500, "Chrome"),
            error("e6", 3_900, "Edge"),
        ],
    );

    let bars = BucketPlan::new(Some(0), Some(4_000), Some(1_000), None).expect("bars");
    let measures = [measure("count", None, None)];
    let group_by = ["browser.name".to_string()];

    let plan = Plan::new(
        RumStream::Errors,
        &AggregateSpec {
            measures: &measures,
            group_by: &group_by,
            bars: Some(bars),
            order_by: None,
            descending: None,
            limit: Some(2),
        },
    )
    .expect("a plan");

    assert!(plan.needs_series());

    let scope = Scope {
        stream: RumStream::Errors,
        from: 0,
        to: 4_000,
        conditions: &[],
    };

    // Chrome has four, Safari and Edge one each, and the two that tie come in the order of their
    // names: the two that rank first are Chrome and Edge.
    let (columns, rows) = run(&context, &plan.first_sql(&scope)).await;
    let groups = plan.groups_of(&columns, &rows).expect("groups");

    assert_eq!(
        groups,
        [
            vec![Some("Chrome".to_string())],
            vec![Some("Edge".to_string())]
        ]
    );

    let (columns, rows) = run(&context, &plan.series_sql(&scope, &groups)).await;
    let answer = plan.answer(&columns, &rows).expect("an answer");

    let seen = answer
        .rows
        .iter()
        .map(|row| {
            (
                row.start_time.expect("a bar"),
                row.group[0].clone().expect("a group"),
                row.values[0].clone(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        seen,
        [
            (0, "Chrome".to_string(), json!(2)),
            (1_000, "Chrome".to_string(), json!(1)),
            (2_000, "Chrome".to_string(), json!(1)),
            (3_000, "Edge".to_string(), json!(1)),
        ]
    );
}

#[tokio::test]
async fn bars_without_groups_count_each_view_once_in_the_bar_it_is_dated_in() {
    let context = pages();

    let bars = BucketPlan::new(Some(0), Some(6_000), Some(2_000), None).expect("bars");
    let measures = [measure("count", None, None)];

    let plan = Plan::new(
        RumStream::Views,
        &AggregateSpec {
            measures: &measures,
            group_by: &[],
            bars: Some(bars),
            order_by: None,
            descending: None,
            limit: None,
        },
    )
    .expect("a plan");

    let scope = Scope {
        stream: RumStream::Views,
        from: 0,
        to: 6_000,
        conditions: &[],
    };

    let (columns, rows) = run(&context, &plan.first_sql(&scope)).await;
    let answer = plan.answer(&columns, &rows).expect("an answer");

    // v-1 (dated 1 000) is one view though it was reported twice; v-2 and v-3 are in the bar
    // from 2 000, v-4 and v-5 in the bar from 4 000.
    assert_eq!(
        answer
            .rows
            .iter()
            .map(|row| (row.start_time, row.values[0].clone()))
            .collect::<Vec<_>>(),
        [
            (Some(0), json!(1)),
            (Some(2_000), json!(2)),
            (Some(4_000), json!(2)),
        ]
    );
}

/// A view of its own session, kept with a share of `rate` percent (none says none).
fn kept(
    event: &'static str,
    session: &'static str,
    group: &'static str,
    rate: Option<i64>,
    lcp: i64,
) -> Written {
    let mut cells = vec![
        ("view.id", Cell::Text(event)),
        ("view.revision", Cell::Int(1)),
        ("session.id", Cell::Text(session)),
        ("view.url_path_group", Cell::Text(group)),
        ("view.error.count", Cell::Int(1)),
        (LCP, Cell::Int(lcp)),
    ];

    if let Some(rate) = rate {
        cells.push(("retention.rate", Cell::Int(rate)));
    }

    written(1_000, event, cells)
}

/// Three sessions of `/`, kept with 25, 25 and 100 percent, and one of `/cart` that says no share.
fn sampled() -> SessionContext {
    context(
        RumStream::Views,
        &[
            kept("e1", "s-1", "/", Some(25), 1_000_000_000),
            kept("e2", "s-2", "/", Some(25), 2_000_000_000),
            kept("e3", "s-3", "/", Some(100), 10_000_000_000),
            kept("e4", "s-4", "/cart", None, 3_000_000_000),
        ],
    )
}

async fn answered(plan: &Plan) -> Vec<(Option<String>, Vec<Value>)> {
    let (columns, rows) = run(&sampled(), &plan.first_sql(&scope(RumStream::Views, &[]))).await;

    plan.answer(&columns, &rows)
        .expect("an answer")
        .rows
        .into_iter()
        .map(|row| (row.group[0].clone(), row.values))
        .collect()
}

#[tokio::test]
async fn an_estimate_counts_each_view_for_as_many_as_the_share_it_was_kept_with_says() {
    let measures = [
        measure("count", None, Some("views")),
        measure("stored", None, Some("rows")),
        measure("count_distinct", Some("session.id"), Some("sessions")),
        measure("count", Some(LCP), Some("with_lcp")),
        measure("sum", Some("view.error.count"), Some("errors")),
        measure("min", Some(LCP), Some("fastest")),
        measure("max", Some(LCP), Some("slowest")),
    ];

    let rows = answered(&plan(
        RumStream::Views,
        &measures,
        &["view.url_path_group"],
        10,
    ))
    .await;
    let of = |group: &str| {
        rows.iter()
            .find(|(name, _)| name.as_deref() == Some(group))
            .map(|(_, values)| values.clone())
            .expect(group)
    };

    // 25 percent counts for four, 100 for one: four, four and one.
    let page = of("/");

    assert_eq!(page[0], json!(9));
    // What is stored is three rows.
    assert_eq!(page[1], json!(3));
    // The three sessions stand for as many as the mean of their rows, three, says.
    assert_eq!(page[2], json!(9));
    assert_eq!(page[3], json!(9));
    // A sum is of the rows that stand for more.
    assert_eq!(page[4].as_f64(), Some(9.0));
    // What was seen is not made larger.
    assert_eq!(page[5], json!(1_000_000_000));
    assert_eq!(page[6], json!(10_000_000_000_i64));

    // A row that says no share counts for itself.
    let cart = of("/cart");

    assert_eq!(
        (cart[0].clone(), cart[1].clone(), cart[2].clone()),
        (json!(1), json!(1), json!(1))
    );
}

#[tokio::test]
async fn the_rows_that_are_stored_are_counted_as_they_are_by_a_plan_that_says_so() {
    let measures = [
        measure("count", None, Some("views")),
        measure("count_distinct", Some("session.id"), Some("sessions")),
    ];

    let group_by = ["view.url_path_group".to_string()];
    let plan = Plan::stored(
        RumStream::Views,
        &AggregateSpec {
            measures: &measures,
            group_by: &group_by,
            bars: None,
            order_by: None,
            descending: None,
            limit: Some(10),
        },
    )
    .expect("a plan");

    let rows = answered(&plan).await;
    let page = rows
        .iter()
        .find(|(name, _)| name.as_deref() == Some("/"))
        .expect("a page");

    assert_eq!(page.1, [json!(3), json!(3)]);
}

#[tokio::test]
async fn an_average_and_a_percentile_are_weighed_by_the_shares() {
    let measures = [
        measure("avg", Some(LCP), Some("average")),
        measure("p50", Some(LCP), Some("median")),
        measure("p90", Some(LCP), Some("ninetieth")),
    ];

    let estimated = answered(&plan(
        RumStream::Views,
        &measures,
        &["view.url_path_group"],
        10,
    ))
    .await;
    let stored = answered(
        &Plan::stored(
            RumStream::Views,
            &AggregateSpec {
                measures: &measures,
                group_by: &["view.url_path_group".to_string()],
                bars: None,
                order_by: None,
                descending: None,
                limit: Some(10),
            },
        )
        .expect("a plan"),
    )
    .await;

    let number = |rows: &[(Option<String>, Vec<Value>)], at: usize| {
        rows.iter()
            .find(|(name, _)| name.as_deref() == Some("/"))
            .and_then(|(_, values)| values[at].as_f64())
            .expect("a number")
    };

    // Three views of 1 s, 2 s and 10 s: the mean is 4.3 s. The first two stand for four each, so the
    // mean of what they stand for is (4 + 8 + 10) / 9 = 2.4 s.
    assert!((number(&stored, 0) - 4_333_333_333.0).abs() < 1.0);
    assert!((number(&estimated, 0) - 22_000_000_000.0 / 9.0).abs() < 1.0);

    // The median of the three that were seen is 2 s; of what they stand for it is between the first two.
    assert!((number(&stored, 1) - 2_000_000_000.0).abs() < 100_000_000.0);
    assert!(
        (1_000_000_000.0..=2_000_000_000.0).contains(&number(&estimated, 1)),
        "{}",
        number(&estimated, 1)
    );

    // Eight of the nine are at or under 2 s, so the ninetieth is beyond it.
    assert!(
        number(&estimated, 2) > 2_000_000_000.0,
        "{}",
        number(&estimated, 2)
    );
}
