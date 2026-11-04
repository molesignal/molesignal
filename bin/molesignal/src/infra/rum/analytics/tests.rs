// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The statements of the analytics run on the engine that answers them in production, over tables
//! with the fixed columns of the RUM streams: that the engine plans them, and that each counts
//! what a person reading the question would count.

use std::sync::Arc;

use datafusion::prelude::SessionContext;
use serde_json::{Value, json};

use super::*;
use crate::{
    domain::rum::event::RumStream,
    infra::{
        query::analyzer::session_context,
        rum::query::execution::{Answer, Cell, Written, answer, table, written},
    },
};

const DAY: i64 = 86_400_000_000;

fn filter(field: &str, op: &str, value: &str) -> Filter {
    Filter {
        field: field.into(),
        op: op.into(),
        value: value.into(),
        quoted: false,
    }
}

fn shop() -> Vec<Filter> {
    vec![filter("application.id", "=", "shop")]
}

fn view(
    id: &'static str,
    ts: i64,
    session: &'static str,
    page: &'static str,
    env: &'static str,
) -> Written {
    written(
        ts,
        id,
        vec![
            ("session.id", Cell::Text(session)),
            ("view.url_path_group", Cell::Text(page)),
            ("application.id", Cell::Text("shop")),
            ("env", Cell::Text(env)),
        ],
    )
}

fn click(
    id: &'static str,
    ts: i64,
    session: &'static str,
    name: &'static str,
    env: &'static str,
) -> Written {
    written(
        ts,
        id,
        vec![
            ("session.id", Cell::Text(session)),
            ("action.target.name", Cell::Text(name)),
            ("application.id", Cell::Text("shop")),
            ("env", Cell::Text(env)),
        ],
    )
}

fn visit(
    id: &'static str,
    ts: i64,
    session: &'static str,
    user: Option<&'static str>,
    anonymous: Option<&'static str>,
) -> Written {
    let mut cells = vec![
        ("session.id", Cell::Text(session)),
        ("application.id", Cell::Text("shop")),
    ];

    if let Some(user) = user {
        cells.push(("usr.id", Cell::Text(user)));
    }

    if let Some(anonymous) = anonymous {
        cells.push(("usr.anonymous_id", Cell::Text(anonymous)));
    }

    written(ts, id, cells)
}

/// The tables the statements read, under the names the engine gives them. The views are the same
/// in both of the tables they are read from.
fn context(views: &[Written], actions: &[Written], sessions: &[Written]) -> SessionContext {
    context_of_revisions(views, views, actions, sessions)
}

/// The same, with views that were written more often than they are the latest: `written` has every
/// revision of a view, `latest` the newest of each.
fn context_of_revisions(
    latest: &[Written],
    written: &[Written],
    actions: &[Written],
    sessions: &[Written],
) -> SessionContext {
    let context = session_context(0);

    for (name, stream, rows) in [
        ("rum_views_latest", RumStream::Views, latest),
        ("rum_views", RumStream::Views, written),
        ("rum_actions", RumStream::Actions, actions),
        ("rum_sessions_latest", RumStream::Sessions, sessions),
    ] {
        context
            .register_table(name, Arc::new(table(stream, rows)))
            .expect("registered once");
    }

    context
}

/// An answer as the engine hands it to the readers: columns, and a row of values for each.
fn values(rows: &Answer) -> (Vec<String>, Vec<Vec<Value>>) {
    let columns = rows
        .first()
        .map(|row| {
            let mut names = row.keys().cloned().collect::<Vec<_>>();

            names.sort();
            names
        })
        .unwrap_or_default();

    let cells = rows
        .iter()
        .map(|row| columns.iter().map(|name| json!(row[name])).collect())
        .collect();

    (columns, cells)
}

fn step(kind: StepKind, field: &str, value: &str) -> Step {
    Step {
        kind,
        name: None,
        filters: vec![filter(field, "=", value)],
    }
}

fn steps() -> Vec<Step> {
    vec![
        step(StepKind::Views, "view.url_path_group", "/"),
        step(StepKind::Views, "view.url_path_group", "/products"),
        step(StepKind::Views, "view.url_path_group", "/checkout"),
        step(StepKind::Actions, "action.target.name", "Pay now"),
    ]
}

/// Five sessions of the shop and one of another application.
fn journeys() -> (Vec<Written>, Vec<Written>) {
    let other = written(
        1_000,
        "o1",
        vec![
            ("session.id", Cell::Text("s9")),
            ("view.url_path_group", Cell::Text("/")),
            ("application.id", Cell::Text("other")),
            ("env", Cell::Text("prod")),
        ],
    );

    let views = vec![
        view("v01", 1_000, "s1", "/", "prod"),
        view("v02", 2_000, "s1", "/products", "prod"),
        view("v03", 3_000, "s1", "/checkout", "prod"),
        view("v04", 1_000, "s2", "/", "prod"),
        view("v05", 2_000, "s2", "/products", "prod"),
        view("v06", 1_000, "s3", "/", "staging"),
        view("v07", 1_500, "s3", "/checkout", "staging"),
        // s4 opened the products before the front page, and again after it.
        view("v08", 500, "s4", "/products", "prod"),
        view("v09", 900, "s4", "/", "prod"),
        view("v10", 5_000, "s4", "/products", "prod"),
        // s5 reloaded the front page.
        view("v11", 1_000, "s5", "/", "prod"),
        view("v12", 1_100, "s5", "/", "prod"),
        view("v13", 1_200, "s5", "/products", "prod"),
        other,
    ];

    let actions = vec![
        click("a1", 4_000, "s1", "Pay now", "prod"),
        // Before the checkout, so it is not the step after it.
        click("a2", 2_500, "s2", "Pay now", "prod"),
    ];

    (views, actions)
}

async fn funnel(spec: &FunnelSpec<'_>, context: &SessionContext) -> Funnel {
    let rows = answer(context, &funnel_sql(spec).expect("a statement")).await;
    let (columns, cells) = values(&rows);

    read_funnel(spec.steps, spec.breakdown.is_some(), &columns, &cells)
}

fn counts(group: &FunnelGroup) -> Vec<u64> {
    group.steps.iter().map(|step| step.sessions).collect()
}

#[tokio::test]
async fn a_funnel_counts_the_sessions_that_did_each_step_after_the_one_before() {
    let (views, actions) = journeys();
    let context = context(&views, &actions, &[]);
    let steps = steps();

    let funnel = funnel(
        &FunnelSpec {
            from: 0,
            to: 1_000_000,
            shared: &shop(),
            steps: &steps,
            window_micros: None,
            breakdown: None,
        },
        &context,
    )
    .await;

    assert_eq!(funnel.groups.len(), 1);

    let group = &funnel.groups[0];

    // Five sessions opened the front page. Four opened the products after it (s4 only the second
    // time), one of them went on to the checkout, and paid after it.
    assert_eq!(counts(group), [5, 4, 1, 1]);
    assert_eq!(group.steps[0].conversion, Some(1.0));
    assert_eq!(group.steps[1].conversion, Some(0.8));
    assert_eq!(group.steps[2].from_previous, Some(0.25));
    assert_eq!(group.steps[3].from_previous, Some(1.0));

    // The sessions took 1000, 1000, 4100 and 200 microseconds to get from the first step to the second.
    let median = group.steps[1].median_ms.expect("a median");

    assert!((0.2..=4.1).contains(&median), "{median}");
    assert!(group.steps[0].median_ms.is_none());
    assert_eq!(group.value, None);
}

#[tokio::test]
async fn a_window_keeps_the_sessions_that_were_quick() {
    let (views, actions) = journeys();
    let context = context(&views, &actions, &[]);
    let steps = steps();

    let funnel = funnel(
        &FunnelSpec {
            from: 0,
            to: 1_000_000,
            shared: &shop(),
            steps: &steps[..2],
            window_micros: Some(1_500),
            breakdown: None,
        },
        &context,
    )
    .await;

    // s4 got to the products 4100 microseconds after the front page: too slow.
    assert_eq!(counts(&funnel.groups[0]), [5, 3]);
}

#[tokio::test]
async fn a_funnel_can_be_broken_down_by_a_field_of_its_first_step() {
    let (views, actions) = journeys();
    let context = context(&views, &actions, &[]);
    let steps = steps();

    let funnel = funnel(
        &FunnelSpec {
            from: 0,
            to: 1_000_000,
            shared: &shop(),
            steps: &steps[..2],
            window_micros: None,
            breakdown: Some("env"),
        },
        &context,
    )
    .await;

    let of = |name: &str| {
        funnel
            .groups
            .iter()
            .find(|group| group.value.as_deref() == Some(name))
            .map(counts)
    };

    assert_eq!(of("prod"), Some(vec![4, 4]));
    assert_eq!(of("staging"), Some(vec![1, 0]));
    // The group with the most sessions comes first.
    assert_eq!(funnel.groups[0].value.as_deref(), Some("prod"));
}

#[tokio::test]
async fn the_time_range_and_the_application_bound_a_funnel() {
    let (views, actions) = journeys();
    let context = context(&views, &actions, &[]);
    let steps = steps();

    let narrow = funnel(
        &FunnelSpec {
            from: 0,
            to: 1_100,
            shared: &shop(),
            steps: &steps[..2],
            window_micros: None,
            breakdown: None,
        },
        &context,
    )
    .await;

    // Only the front page is early enough, and every session had opened it by then.
    assert_eq!(counts(&narrow.groups[0]), [5, 0]);

    let everything = funnel(
        &FunnelSpec {
            from: 0,
            to: 1_000_000,
            shared: &[],
            steps: &steps[..2],
            window_micros: None,
            breakdown: None,
        },
        &context,
    )
    .await;

    // Without the application the session of the other one counts too.
    assert_eq!(counts(&everything.groups[0]), [6, 4]);
}

#[test]
fn a_funnel_is_checked_before_it_is_asked() {
    let steps = steps();

    let ask = |steps: &[Step], from: i64, to: i64, breakdown: Option<&str>| {
        funnel_sql(&FunnelSpec {
            from,
            to,
            shared: &[],
            steps,
            window_micros: None,
            breakdown,
        })
    };

    assert!(
        ask(&steps[..1], 0, 10, None).is_err(),
        "one step is no funnel"
    );
    assert!(ask(&vec![steps[0].clone(); 9], 0, 10, None).is_err());
    assert!(ask(&steps[..2], 10, 10, None).is_err());
    assert!(ask(&steps[..2], 0, 10, None).is_ok());

    let unknown = Step {
        kind: StepKind::Views,
        name: None,
        filters: vec![filter("view.nope", "=", "x")],
    };

    assert!(ask(&[unknown.clone(), unknown], 0, 10, None).is_err());
    assert!(
        ask(&steps[..2], 0, 10, Some("view.loading_time")).is_err(),
        "a number does not tell sessions apart"
    );
    assert!(ask(&steps[..2], 0, 10, Some("nope")).is_err());
}

#[test]
fn a_step_is_named_by_its_name_or_by_what_it_looks_for() {
    let named = Step {
        kind: StepKind::Views,
        name: Some(" Checkout ".into()),
        filters: vec![filter("view.url_path_group", "=", "/checkout")],
    };

    assert_eq!(label(&named, 2), "Checkout");
    assert_eq!(label(&steps()[1], 1), "/products");

    let bare = Step {
        kind: StepKind::Actions,
        name: None,
        filters: Vec::new(),
    };

    assert_eq!(label(&bare, 3), "#4");
}

/// The views of the journeys written once more: the SDK sends a view again each time its numbers
/// change, at the time the view began.
fn views_again() -> Vec<Written> {
    vec![
        view("a01", 1_000, "s1", "/", "prod"),
        view("a02", 2_000, "s1", "/products", "prod"),
        view("a03", 3_000, "s1", "/checkout", "prod"),
        view("a04", 1_000, "s2", "/", "prod"),
        view("a05", 2_000, "s2", "/products", "prod"),
        view("a06", 1_000, "s3", "/", "staging"),
        view("a07", 1_500, "s3", "/checkout", "staging"),
        view("a08", 500, "s4", "/products", "prod"),
        view("a09", 900, "s4", "/", "prod"),
        view("a10", 5_000, "s4", "/products", "prod"),
        view("a11", 1_000, "s5", "/", "prod"),
        view("a12", 1_100, "s5", "/", "prod"),
        view("a13", 1_200, "s5", "/products", "prod"),
    ]
}

#[tokio::test]
async fn the_views_as_they_were_written_count_the_same_sessions_as_the_latest_of_them() {
    let (views, actions) = journeys();
    let mut written = journeys().0;

    written.extend(views_again());

    let context = context_of_revisions(&views, &written, &actions, &[]);
    let steps = steps();

    let spec = FunnelSpec {
        from: 0,
        to: 1_000_000,
        shared: &shop(),
        steps: &steps,
        window_micros: None,
        breakdown: None,
    };

    // The statement reads the views as written: every one of them is there twice.
    assert!(funnel_sql(&spec).unwrap().contains("FROM rum_views WHERE"));
    assert!(!funnel_sql(&spec).unwrap().contains("rum_views_latest"));

    // The same journeys, with every view written once: what the statement says is the same.
    let once = self::context(&views, &actions, &[]);
    let funnel_of = funnel(&spec, &context).await;

    assert_eq!(counts(&funnel_of.groups[0]), [5, 4, 1, 1]);
    assert_eq!(funnel_of, funnel(&spec, &once).await);

    let ask = PathsSpec {
        from: 0,
        to: 1_000_000,
        shared: &shop(),
        anchor: Some("/"),
        backward: false,
        depth: 2,
        by: "view.url_path_group",
    };

    let found = paths(&ask, &context).await;

    // A page that was written twice is one page: the second row of it is the page again at once.
    assert_eq!(found.sessions, 5);
    assert_eq!(link(&found, 0, "/", "/products"), 4);
    assert_eq!(found, paths(&ask, &once).await);
}

#[test]
fn the_latest_views_are_read_only_when_what_is_asked_of_them_can_differ_between_revisions() {
    let steps = |filters: Vec<Filter>| {
        vec![
            step(StepKind::Views, "view.url_path_group", "/"),
            Step {
                kind: StepKind::Views,
                name: None,
                filters,
            },
        ]
    };

    let sql = |steps: &[Step], breakdown: Option<&str>| {
        funnel_sql(&FunnelSpec {
            from: 0,
            to: 1_000_000,
            shared: &shop(),
            steps,
            window_micros: None,
            breakdown,
        })
        .unwrap()
    };

    // Where a view was, and who it belongs to, is said by every revision of it.
    let alike = sql(
        &steps(vec![filter("view.url_path_group", "=", "/cart")]),
        Some("browser.name"),
    );

    assert!(!alike.contains("rum_views_latest"), "{alike}");

    // How many errors it had is said by the newest, and only by it.
    let counted = sql(&steps(vec![filter("view.error.count", ">", "0")]), None);

    assert_eq!(
        counted.matches("FROM rum_views_latest").count(),
        1,
        "{counted}"
    );
    assert_eq!(
        counted.matches("FROM rum_views WHERE").count(),
        1,
        "{counted}"
    );

    // Neither is who the person was, which is learned when they sign in.
    let signed = sql(
        &steps(vec![filter("view.url_path_group", "=", "/cart")]),
        Some("usr.id"),
    );

    assert_eq!(
        signed.matches("FROM rum_views_latest").count(),
        1,
        "{signed}"
    );

    let paths = |shared: Vec<Filter>| {
        paths_sql(&PathsSpec {
            from: 0,
            to: 1_000_000,
            shared: &shared,
            anchor: None,
            backward: false,
            depth: 2,
            by: "view.url_path_group",
        })
        .unwrap()
    };

    assert!(paths(shop()).contains("FROM rum_views WHERE"));
    assert!(paths(vec![filter("view.error.count", ">", "0")]).contains("FROM rum_views_latest"));
}

async fn paths(spec: &PathsSpec<'_>, context: &SessionContext) -> Paths {
    let rows = answer(context, &paths_sql(spec).expect("a statement")).await;
    let (columns, cells) = values(&rows);

    read_paths(spec.depth, spec.backward, &columns, &cells)
}

fn node<'a>(paths: &'a Paths, column: usize, page: &str) -> &'a PathNode {
    paths
        .nodes
        .iter()
        .find(|node| node.column == column && node.page == page)
        .unwrap_or_else(|| panic!("no {page} in column {column}: {:?}", paths.nodes))
}

fn link(paths: &Paths, column: usize, source: &str, target: &str) -> u64 {
    paths
        .links
        .iter()
        .find(|link| link.column == column && link.source == source && link.target == target)
        .map_or(0, |link| link.sessions)
}

#[tokio::test]
async fn paths_follow_the_pages_a_session_opened_after_a_page() {
    let (views, actions) = journeys();
    let context = context(&views, &actions, &[]);

    let found = paths(
        &PathsSpec {
            from: 0,
            to: 1_000_000,
            shared: &shop(),
            anchor: Some("/"),
            backward: false,
            depth: 2,
            by: "view.url_path_group",
        },
        &context,
    )
    .await;

    assert_eq!(found.columns, 3);
    assert_eq!(found.sessions, 5);
    assert_eq!(node(&found, 0, "/").sessions, 5);
    assert_eq!(node(&found, 1, "/products").sessions, 4);
    assert_eq!(node(&found, 1, "/checkout").sessions, 1);
    assert_eq!(node(&found, 2, "/checkout").sessions, 1);

    assert_eq!(link(&found, 0, "/", "/products"), 4);
    assert_eq!(link(&found, 0, "/", "/checkout"), 1);
    assert_eq!(link(&found, 1, "/products", "/checkout"), 1);

    // Three of the four that opened the products went no further.
    assert_eq!(node(&found, 1, "/products").ends, 3);
    assert_eq!(node(&found, 0, "/").ends, 0);
}

#[tokio::test]
async fn a_page_opened_again_at_once_counts_once() {
    let (views, actions) = journeys();
    let context = context(&views, &actions, &[]);

    let found = paths(
        &PathsSpec {
            from: 0,
            to: 1_000_000,
            shared: &shop(),
            anchor: Some("/"),
            backward: false,
            depth: 1,
            by: "view.url_path_group",
        },
        &context,
    )
    .await;

    // s5 reloaded the front page; its next page is still the products.
    assert_eq!(node(&found, 0, "/").sessions, 5);
    assert_eq!(link(&found, 0, "/", "/products"), 4);
}

#[tokio::test]
async fn paths_can_be_followed_back_from_a_page() {
    let (views, actions) = journeys();
    let context = context(&views, &actions, &[]);

    let found = paths(
        &PathsSpec {
            from: 0,
            to: 1_000_000,
            shared: &shop(),
            anchor: Some("/checkout"),
            backward: true,
            depth: 2,
            by: "view.url_path_group",
        },
        &context,
    )
    .await;

    assert!(found.backward);
    assert_eq!(found.sessions, 2);

    // The columns run from the earliest page to the one the paths end at.
    assert_eq!(node(&found, 2, "/checkout").sessions, 2);
    assert_eq!(node(&found, 1, "/products").sessions, 1);
    assert_eq!(node(&found, 1, "/").sessions, 1);
    assert_eq!(node(&found, 0, "/").sessions, 1);

    assert_eq!(link(&found, 1, "/products", "/checkout"), 1);
    assert_eq!(link(&found, 1, "/", "/checkout"), 1);
    assert_eq!(link(&found, 0, "/", "/products"), 1);

    // The paths of the first column began there, those of the products did not.
    assert_eq!(node(&found, 0, "/").ends, 1);
    assert_eq!(node(&found, 1, "/products").ends, 0);
    assert_eq!(node(&found, 1, "/").ends, 1);
}

#[tokio::test]
async fn without_a_page_paths_begin_with_the_first_page_of_each_session() {
    let (views, actions) = journeys();
    let context = context(&views, &actions, &[]);

    let found = paths(
        &PathsSpec {
            from: 0,
            to: 1_000_000,
            shared: &shop(),
            anchor: None,
            backward: false,
            depth: 1,
            by: "view.url_path_group",
        },
        &context,
    )
    .await;

    // s4 began with the products, the other four with the front page.
    assert_eq!(node(&found, 0, "/").sessions, 4);
    assert_eq!(node(&found, 0, "/products").sessions, 1);
    assert_eq!(found.sessions, 5);
}

#[test]
fn a_column_names_its_most_visited_pages_and_pools_the_rest() {
    let columns = ["kind", "depth", "source", "target", "sessions"]
        .map(String::from)
        .to_vec();

    let rows = (0..12)
        .map(|index| {
            vec![
                json!("node"),
                json!(0),
                json!(format!("/page-{index:02}")),
                json!(""),
                json!(100 - index),
            ]
        })
        .collect::<Vec<_>>();

    let found = read_paths(1, false, &columns, &rows);
    let first = found
        .nodes
        .iter()
        .filter(|node| node.column == 0)
        .collect::<Vec<_>>();

    assert_eq!(first.len(), PER_LEVEL + 1);
    assert_eq!(first[0].page, "/page-00");
    // The rest stands last, though it holds more sessions than any of the pages.
    assert_eq!(first[PER_LEVEL].page, OTHER);

    let rest = first
        .iter()
        .find(|node| node.page == OTHER)
        .expect("the rest");

    // Pages 8 to 11 had 92, 91, 90 and 89.
    assert_eq!(rest.sessions, 92 + 91 + 90 + 89);
}

#[test]
fn paths_are_checked_before_they_are_asked() {
    let spec = |depth, by| PathsSpec {
        from: 0,
        to: 10,
        shared: &[],
        anchor: None,
        backward: false,
        depth,
        by,
    };

    assert!(paths_sql(&spec(0, "view.url_path_group")).is_err());
    assert!(paths_sql(&spec(MAX_DEPTH + 1, "view.url_path_group")).is_err());
    assert!(paths_sql(&spec(2, "view.loading_time")).is_err());
    assert!(paths_sql(&spec(2, "view.name")).is_ok());
}

async fn retention(spec: &RetentionSpec<'_>, context: &SessionContext) -> Retention {
    let rows = answer(context, &retention_sql(spec).expect("a statement")).await;
    let (columns, cells) = values(&rows);

    read_retention(spec, &columns, &cells)
}

fn day(index: i64) -> i64 {
    (20_000 + index) * DAY + 1_000
}

#[tokio::test]
async fn retention_counts_who_came_back_in_each_period_after_the_first() {
    let sessions = [
        visit("t01", day(0), "a", Some("u1"), None),
        visit("t02", day(1), "b", Some("u1"), None),
        visit("t03", day(3), "c", Some("u1"), None),
        visit("t04", day(0), "d", Some("u2"), None),
        visit("t05", day(2), "e", Some("u2"), None),
        visit("t06", day(1), "f", Some("u3"), None),
        visit("t07", day(2), "g", Some("u3"), None),
        // A visitor with no id is told by the anonymous one.
        visit("t08", day(0), "h", None, Some("anon-1")),
        visit("t09", day(1), "i", None, Some("anon-1")),
        // Nobody can be told from this one.
        visit("t10", day(0), "j", None, None),
        // A second visit on the same day is the same person once.
        visit("t11", day(0), "k", Some("u1"), None),
    ];

    let context = context(&[], &[], &sessions);

    let found = retention(
        &RetentionSpec {
            from: 20_000 * DAY,
            to: 20_010 * DAY,
            shared: &shop(),
            period: Period::Day,
            periods: 3,
        },
        &context,
    )
    .await;

    assert_eq!(found.cohorts.len(), 2);

    let first = &found.cohorts[0];

    assert_eq!(first.start_time, 20_000 * DAY);
    assert_eq!(first.users, 3);
    assert_eq!(first.returned, [Some(2), Some(1), Some(1)]);

    let second = &found.cohorts[1];

    assert_eq!(second.start_time, 20_001 * DAY);
    assert_eq!(second.users, 1);
    assert_eq!(second.returned, [Some(1), Some(0), Some(0)]);
}

#[tokio::test]
async fn a_period_that_has_not_ended_by_the_end_of_the_range_has_no_number() {
    let sessions = [
        visit("t01", day(0), "a", Some("u1"), None),
        visit("t02", day(1), "b", Some("u1"), None),
    ];

    let context = context(&[], &[], &sessions);

    let found = retention(
        &RetentionSpec {
            from: 20_000 * DAY,
            to: 20_002 * DAY,
            shared: &shop(),
            period: Period::Day,
            periods: 3,
        },
        &context,
    )
    .await;

    // The second day ends with the range; the third and fourth have not been.
    assert_eq!(found.cohorts[0].returned, [Some(1), None, None]);
}

#[test]
fn weeks_begin_on_monday() {
    // 2026-10-19 is a Monday, the 20 745th day since the epoch.
    let monday = 20_745;

    for offset in 0..7 {
        let at = (monday + offset) * DAY;
        let index = (at / DAY + 3) / 7;

        assert_eq!(Period::Week.start(index), monday * DAY, "day {offset}");
    }

    assert_eq!(Period::Week.length(), 7 * DAY);
}

#[test]
fn retention_is_checked_before_it_is_asked() {
    let spec = |periods, from, to| RetentionSpec {
        from,
        to,
        shared: &[],
        period: Period::Week,
        periods,
    };

    assert!(retention_sql(&spec(0, 0, 10)).is_err());
    assert!(retention_sql(&spec(31, 0, 10)).is_err());
    assert!(retention_sql(&spec(4, 10, 10)).is_err());
    assert!(retention_sql(&spec(4, 0, 10)).is_ok());
}
