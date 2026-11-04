// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashSet;

use serde_json::Value;

use super::*;
use crate::{
    app::dashboard::{
        contract_registry::builtin_dashboard_contract_resolver,
        validation::{DashboardValidationMode, ensure_dashboard_model_with},
    },
    domain::rum::event::RumStream,
};

fn models() -> Vec<(RumDashboard, Value)> {
    RumDashboard::ALL
        .into_iter()
        .map(|dashboard| (dashboard, dashboard.model(&Id::from_string("dashboard-1"))))
        .collect()
}

fn panels(model: &Value) -> Vec<&Value> {
    model["elements"].as_array().unwrap().iter().collect()
}

/// The statement a panel runs, with the variables a person has chosen.
fn bound(sql: &str, application: &str, environment: &str, interval: &str) -> String {
    sql.replace("${application:sqlstring}", &format!("'{application}'"))
        .replace("${environment:sqlstring}", &format!("'{environment}'"))
        .replace("${interval}", interval)
}

fn statement(panel: &Value) -> &str {
    panel["queries"][0]["query"]["statement"].as_str().unwrap()
}

#[tokio::test]
async fn a_dashboard_meets_the_dashboard_contract() {
    let contracts = builtin_dashboard_contract_resolver()
        .active()
        .await
        .unwrap();

    for (dashboard, model) in models() {
        ensure_dashboard_model_with(
            &model,
            DashboardValidationMode::Native,
            &contracts.model_validator,
            &contracts.manifest,
        )
        .unwrap_or_else(|error| panic!("{}: {error:?}", dashboard.uid()));
    }
}

#[test]
fn a_dashboard_is_read_only_and_tagged_as_built_in() {
    for (dashboard, model) in models() {
        assert_eq!(model["editable"], false, "{}", dashboard.uid());
        assert_eq!(model["uid"], dashboard.uid());
        assert_eq!(model["title"], dashboard.title());
        assert_eq!(model["tags"], serde_json::json!(["内置", "rum"]));
        assert_eq!(installed_version(&model), VERSION);
    }

    assert_eq!(installed_version(&serde_json::json!({})), 0);
}

#[test]
fn what_a_dashboard_asks_is_asked_of_the_application_and_the_environment_in_front() {
    for (dashboard, model) in models() {
        let names = model["variables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|variable| variable["name"].as_str().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            ["application", "environment", "interval"],
            "{}",
            dashboard.uid()
        );

        for panel in panels(&model) {
            let sql = statement(panel);

            assert!(
                sql.contains("${application:sqlstring}"),
                "{}: {sql}",
                panel["id"]
            );
            assert!(
                sql.contains("${environment:sqlstring}"),
                "{}: {sql}",
                panel["id"]
            );
        }
    }
}

#[test]
fn the_ids_of_panels_are_unique_within_a_dashboard() {
    for (dashboard, model) in models() {
        let ids = panels(&model)
            .iter()
            .map(|panel| panel["id"].as_str().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(
            ids.iter().collect::<HashSet<_>>().len(),
            ids.len(),
            "{}",
            dashboard.uid()
        );
    }
}

#[test]
fn a_panel_reads_the_streams_of_rum_and_names_the_one_it_is_about() {
    for (dashboard, model) in models() {
        for panel in panels(&model) {
            let sql = bound(statement(panel), "shop", "prod", "15 minutes");
            let tables = query_language::extract_referenced_tables(&sql)
                .unwrap_or_else(|error| panic!("{}: {error}: {sql}", panel["id"]));

            assert!(!tables.is_empty(), "{}", panel["id"]);

            // `rum_views_latest` is the newest revisions of `rum_views`.
            let streams = tables
                .iter()
                .map(|table| table.name.strip_suffix("_latest").unwrap_or(&table.name))
                .collect::<Vec<_>>();

            for stream in &streams {
                assert!(
                    RumStream::from_name(stream).is_some(),
                    "{} reads {stream}",
                    panel["id"]
                );
            }

            let query = &panel["queries"][0]["query"];

            assert_eq!(query["streamType"], "logs", "{}", panel["id"]);
            assert_eq!(
                query["streamName"].as_str(),
                streams.first().copied(),
                "{} in {}",
                panel["id"],
                dashboard.uid()
            );
        }
    }
}

#[test]
fn a_versioned_stream_is_read_through_its_current_versions_only() {
    for (_, model) in models() {
        for panel in panels(&model) {
            let sql = statement(panel);

            for stream in ["rum_views", "rum_sessions"] {
                let plain = format!("FROM {stream} ");

                assert!(
                    !sql.contains(&plain),
                    "{} reads every revision of {stream}: {sql}",
                    panel["id"]
                );
            }
        }
    }
}

#[test]
fn a_chart_over_time_names_its_time_column_and_asks_for_a_time_series() {
    for (_, model) in models() {
        for panel in panels(&model) {
            if panel["visualization"]["type"] != "time_series" {
                continue;
            }

            assert_eq!(panel["queries"][0]["format"], "time_series");
            assert!(
                statement(panel).contains(r#" AS "time""#),
                "{}",
                panel["id"]
            );
        }
    }
}

#[test]
fn the_panels_do_not_overlap() {
    for (dashboard, model) in models() {
        let boxes = panels(&model)
            .iter()
            .map(|panel| {
                let at = &panel["gridPos"];
                let get = |key: &str| at[key].as_u64().unwrap();

                (
                    get("x"),
                    get("y"),
                    get("w"),
                    get("h"),
                    panel["id"].to_string(),
                )
            })
            .collect::<Vec<_>>();

        for (index, left) in boxes.iter().enumerate() {
            assert!(left.0 + left.2 <= 24, "{} leaves the grid", left.4);

            for right in &boxes[index + 1..] {
                let apart = left.0 + left.2 <= right.0
                    || right.0 + right.2 <= left.0
                    || left.1 + left.3 <= right.1
                    || right.1 + right.3 <= left.1;

                assert!(
                    apart,
                    "{} and {} overlap in {}",
                    left.4,
                    right.4,
                    dashboard.uid()
                );
            }
        }
    }
}

#[test]
fn the_id_of_a_dashboard_is_made_of_its_uid_and_the_organization() {
    let org = Id::new();

    assert_eq!(
        RumDashboard::Browser.id(&org).0,
        format!("rum-browser-{}", org.0)
    );

    // It is stored in a column of 64 characters.
    for dashboard in RumDashboard::ALL {
        assert!(dashboard.id(&org).0.len() <= 64, "{}", dashboard.uid());
    }

    assert_ne!(
        RumDashboard::Browser.id(&org),
        RumDashboard::Mobile.id(&org)
    );
    assert_ne!(
        RumDashboard::Browser.id(&org),
        RumDashboard::Browser.id(&Id::new())
    );
}

#[test]
fn a_platform_has_the_dashboard_of_its_kind() {
    assert_eq!(
        RumDashboard::of(RumPlatform::Browser),
        Some(RumDashboard::Browser)
    );

    for platform in [
        RumPlatform::Android,
        RumPlatform::Ios,
        RumPlatform::Flutter,
        RumPlatform::ReactNative,
    ] {
        assert_eq!(RumDashboard::of(platform), Some(RumDashboard::Mobile));
    }

    assert_eq!(RumDashboard::of(RumPlatform::Other), None);
}

/// The content the organizations have is the content of the version they have: a change of a
/// panel, a variable or a title without a new `VERSION` would never reach them.
#[test]
fn the_content_of_a_version_does_not_change() {
    let mut digest = blake3::Hasher::new();

    for dashboard in RumDashboard::ALL {
        let model = dashboard.model(&Id::from_string("dashboard-1"));

        digest.update(serde_json::to_string(&model).unwrap().as_bytes());
    }

    assert_eq!(
        digest.finalize().to_hex().as_str(),
        "41a071f0a13e69c77611c7c454b433a0d3c501caccf6d034a0b871cbf12f938c",
        "the built-in RUM dashboards changed: raise `VERSION` and pin the new digest"
    );
}

/// The SQL of the panels, run on the real schemas of the streams with a few rows.
mod sql {
    use std::sync::Arc;

    use arrow::{
        array::{
            ArrayRef, BooleanArray, Float64Array, Int64Array, RecordBatch, StringArray,
            TimestampMicrosecondArray,
        },
        datatypes::SchemaRef,
    };
    use datafusion::{datasource::MemTable, prelude::SessionContext};
    use serde_json::{Value, json};

    use super::*;
    use crate::{
        domain::rum::event::{RumStream, stream_schema},
        infra::{
            query::{
                analyzer::session_context, arrow_json::batches_to_json,
                current_views::CURRENT_VIEWS,
            },
            rum::query::{AggregateSpec, Condition, Filter, MeasureSpec, Plan, Scope},
            storage::arrow_schema::{align_batch_to_schema, to_arrow},
        },
        shared::ids::Id,
    };

    const HOUR: i64 = 3_600_000_000;
    const T: i64 = HOUR;

    enum Col {
        Time(Vec<i64>),
        Text(Vec<Option<&'static str>>),
        Int(Vec<Option<i64>>),
        Float(Vec<Option<f64>>),
        Flag(Vec<Option<bool>>),
    }

    fn text(values: &[&'static str]) -> Col {
        Col::Text(values.iter().map(|value| Some(*value)).collect())
    }

    fn same(count: usize, value: &'static str) -> Col {
        Col::Text(vec![Some(value); count])
    }

    fn int(values: &[i64]) -> Col {
        Col::Int(values.iter().map(|value| Some(*value)).collect())
    }

    fn float(values: &[f64]) -> Col {
        Col::Float(values.iter().map(|value| Some(*value)).collect())
    }

    /// The rows of a stream: the columns that are given, and nothing in the others.
    fn table(stream: RumStream, columns: Vec<(&str, Col)>) -> (SchemaRef, Vec<RecordBatch>) {
        let target = to_arrow(&stream_schema(stream));

        if columns.is_empty() {
            return (target, Vec::new());
        }

        let arrays = columns.into_iter().map(|(name, column)| {
            let array: ArrayRef = match column {
                Col::Time(values) => {
                    Arc::new(TimestampMicrosecondArray::from(values).with_timezone("UTC"))
                }
                Col::Text(values) => Arc::new(StringArray::from(values)),
                Col::Int(values) => Arc::new(Int64Array::from(values)),
                Col::Float(values) => Arc::new(Float64Array::from(values)),
                Col::Flag(values) => Arc::new(BooleanArray::from(values)),
            };

            (name, array)
        });

        let batch = RecordBatch::try_from_iter(arrays).unwrap();

        (
            target.clone(),
            vec![align_batch_to_schema(&batch, &target).unwrap()],
        )
    }

    fn views() -> (SchemaRef, Vec<RecordBatch>) {
        table(
            RumStream::Views,
            vec![
                (
                    "_timestamp",
                    Col::Time(vec![T + 1, T + 1, T + 1, T + 2, T + HOUR, T + 3]),
                ),
                ("_event_id", text(&["e0", "e1", "e2", "e3", "e4", "e5"])),
                (
                    "application.id",
                    text(&["shop", "shop", "shop", "shop", "shop", "other"]),
                ),
                (
                    "env",
                    text(&["prod", "prod", "prod", "prod", "staging", "prod"]),
                ),
                ("source", same(6, "browser")),
                ("session.id", text(&["s1", "s1", "s1", "s2", "s3", "s9"])),
                ("view.id", text(&["v1", "v1", "v1", "v2", "v4", "v3"])),
                ("view.revision", int(&[1, 2, 3, 1, 1, 1])),
                (
                    "version",
                    text(&["2.4.1", "2.4.1", "2.4.1", "2.4.1", "2.4.0", "1.0"]),
                ),
                (
                    "view.url_path_group",
                    text(&["/", "/", "/", "/cart", "/checkout", "/"]),
                ),
                (
                    "view.performance.lcp.timestamp",
                    int(&[
                        3_000_000_000,
                        2_000_000_000,
                        2_400_000_000,
                        1_000_000_000,
                        5_000_000_000,
                        9_000_000_000,
                    ]),
                ),
                (
                    "view.performance.inp.duration",
                    int(&[
                        300_000_000,
                        200_000_000,
                        180_000_000,
                        100_000_000,
                        600_000_000,
                        900_000_000,
                    ]),
                ),
                (
                    "view.performance.cls.score",
                    float(&[0.30, 0.20, 0.10, 0.05, 0.40, 0.90]),
                ),
                ("view.error.count", int(&[0, 1, 0, 0, 1, 5])),
            ],
        )
    }

    fn sessions() -> (SchemaRef, Vec<RecordBatch>) {
        table(
            RumStream::Sessions,
            vec![
                (
                    "_timestamp",
                    Col::Time(vec![T + 1, T + 1, T + 2, T + HOUR, T + 3]),
                ),
                ("_event_id", text(&["e0", "e1", "e2", "e3", "e4"])),
                (
                    "application.id",
                    text(&["shop", "shop", "shop", "shop", "other"]),
                ),
                ("env", text(&["prod", "prod", "prod", "staging", "prod"])),
                ("source", same(5, "browser")),
                ("session.id", text(&["s1", "s1", "s2", "s3", "s9"])),
                ("session.revision", int(&[1, 2, 1, 1, 1])),
                (
                    "version",
                    text(&["2.4.1", "2.4.1", "2.4.1", "2.4.0", "1.0"]),
                ),
                ("session.error.count", int(&[0, 1, 0, 1, 0])),
                ("session.frustration.count", int(&[0, 1, 0, 0, 0])),
                ("session.crash.count", int(&[0, 0, 0, 0, 0])),
                (
                    "geo.country",
                    text(&["中国", "中国", "美国", "美国", "日本"]),
                ),
                (
                    "browser.name",
                    text(&["Chrome", "Chrome", "Safari", "Chrome", "Edge"]),
                ),
            ],
        )
    }

    fn errors() -> (SchemaRef, Vec<RecordBatch>) {
        table(
            RumStream::Errors,
            vec![
                ("_timestamp", Col::Time(vec![T + 1, T + 2, T + 3])),
                ("application.id", same(3, "shop")),
                ("env", same(3, "prod")),
                ("session.id", text(&["s1", "s1", "s2"])),
                ("error.source", text(&["source", "network", "network"])),
                (
                    "error.is_crash",
                    Col::Flag(vec![Some(false), Some(false), Some(false)]),
                ),
            ],
        )
    }

    fn actions() -> (SchemaRef, Vec<RecordBatch>) {
        table(
            RumStream::Actions,
            vec![
                ("_timestamp", Col::Time(vec![T + 1, T + 2, T + 3])),
                ("application.id", same(3, "shop")),
                ("env", same(3, "prod")),
                ("session.id", text(&["s3", "s3", "s1"])),
                ("view.url_path_group", same(3, "/checkout")),
                ("action.target.name", same(3, "提交订单")),
                ("action.frustration.count", int(&[2, 1, 1])),
            ],
        )
    }

    /// Ten calls to one host, two of them failed, all of them 400 ms.
    fn resources() -> (SchemaRef, Vec<RecordBatch>) {
        table(
            RumStream::Resources,
            vec![
                ("_timestamp", Col::Time((1..=10).map(|n| T + n).collect())),
                ("application.id", same(10, "shop")),
                ("env", same(10, "prod")),
                ("resource.type", same(10, "fetch")),
                ("resource.url_host", same(10, "api.shop.example")),
                ("resource.duration", int(&[400_000_000; 10])),
                (
                    "resource.status_code",
                    int(&[200, 200, 200, 200, 200, 200, 200, 200, 500, 0]),
                ),
            ],
        )
    }

    fn context() -> SessionContext {
        let ctx = session_context(0);

        let streams = [
            (RumStream::Views, views()),
            (RumStream::Sessions, sessions()),
            (RumStream::Errors, errors()),
            (RumStream::Actions, actions()),
            (RumStream::Resources, resources()),
        ];

        for (stream, (schema, batches)) in streams {
            let table = Arc::new(MemTable::try_new(schema, vec![batches]).unwrap());

            ctx.register_table(stream.name(), table.clone()).unwrap();

            for view in CURRENT_VIEWS.iter().filter(|view| view.stream == stream) {
                view.register(&ctx, table.clone()).unwrap();
            }
        }

        ctx
    }

    fn panel<'a>(model: &'a Value, id: &str) -> &'a Value {
        model["elements"]
            .as_array()
            .unwrap()
            .iter()
            .find(|panel| panel["id"] == id)
            .unwrap_or_else(|| panic!("no panel {id}"))
    }

    async fn run(
        ctx: &SessionContext,
        dashboard: RumDashboard,
        id: &str,
        environment: &str,
        interval: &str,
    ) -> Vec<Vec<Value>> {
        let model = dashboard.model(&Id::from_string("dashboard-1"));
        let sql = bound(statement(panel(&model, id)), "shop", environment, interval);

        let batches = ctx
            .sql(&sql)
            .await
            .unwrap_or_else(|error| panic!("{id}: {error}\n{sql}"))
            .collect()
            .await
            .unwrap_or_else(|error| panic!("{id}: {error}\n{sql}"));

        batches_to_json(&batches).1
    }

    fn number(rows: &[Vec<Value>]) -> f64 {
        rows[0][0].as_f64().unwrap()
    }

    #[tokio::test]
    async fn every_panel_runs_on_the_real_schemas() {
        let ctx = context();

        for dashboard in RumDashboard::ALL {
            let model = dashboard.model(&Id::from_string("dashboard-1"));

            for element in panels(&model) {
                let id = element["id"].as_str().unwrap();

                // Whatever the choice of the person, it is a statement the engine plans and runs.
                for (environment, interval) in [
                    ("全部", "15 minutes"),
                    ("prod", "1 hour"),
                    ("", "5 minutes"),
                ] {
                    run(&ctx, dashboard, id, environment, interval).await;
                }
            }
        }
    }

    #[tokio::test]
    async fn the_figures_are_those_of_the_newest_revision_of_each_session() {
        let ctx = context();
        let browser = RumDashboard::Browser;

        let sessions = run(&ctx, browser, "sessions", "全部", "1 hour").await;
        assert_eq!(sessions, vec![vec![json!(3)]]);

        // `s1` had no error at its first revision and has one at its second.
        let clean = number(&run(&ctx, browser, "clean-sessions", "全部", "1 hour").await);
        assert!((clean - 100.0 / 3.0).abs() < 1e-9, "{clean}");

        let frustrated = number(&run(&ctx, browser, "frustrated-sessions", "全部", "1 hour").await);
        assert!((frustrated - 100.0 / 3.0).abs() < 1e-9, "{frustrated}");

        // An environment narrows every figure.
        assert_eq!(
            run(&ctx, browser, "sessions", "staging", "1 hour").await,
            vec![vec![json!(1)]]
        );
        assert_eq!(
            number(&run(&ctx, browser, "clean-sessions", "staging", "1 hour").await),
            0.0
        );

        // Nothing of another application, and nothing at all of an application nobody sent.
        let model = browser.model(&Id::from_string("dashboard-1"));
        let sql = bound(
            statement(panel(&model, "sessions")),
            "nobody",
            "全部",
            "1 hour",
        );
        let none = ctx.sql(&sql).await.unwrap().collect().await.unwrap();

        assert_eq!(batches_to_json(&none).1, vec![vec![json!(0)]]);
    }

    #[tokio::test]
    async fn a_vital_is_the_percentile_of_the_newest_revisions_that_have_a_value() {
        let ctx = context();
        let browser = RumDashboard::Browser;

        // One view in staging: its figure is the percentile of anything.
        assert_eq!(
            number(&run(&ctx, browser, "lcp", "staging", "1 hour").await),
            5.0
        );
        assert_eq!(
            number(&run(&ctx, browser, "inp", "staging", "1 hour").await),
            600.0
        );
        assert_eq!(
            number(&run(&ctx, browser, "cls", "staging", "1 hour").await),
            0.4
        );

        // In prod the first revision of `v1` (3 s) is not the view any more: the slowest of the
        // two views is 2.4 s, so the percentile cannot be above it.
        let lcp = number(&run(&ctx, browser, "lcp", "prod", "1 hour").await);
        assert!(lcp > 1.0 && lcp <= 2.4 + 1e-9, "{lcp}");
    }

    #[tokio::test]
    async fn a_table_counts_what_is_current() {
        let ctx = context();
        let browser = RumDashboard::Browser;

        let pages = run(&ctx, browser, "pages", "prod", "1 hour").await;
        let counted = pages
            .iter()
            .map(|row| {
                (
                    row[0].as_str().unwrap().to_string(),
                    row[1].as_i64().unwrap(),
                    row[4].as_f64().unwrap(),
                )
            })
            .collect::<Vec<_>>();

        // `v1` is one view on `/`, and the error of its second revision is gone with it.
        assert!(counted.contains(&("/".to_string(), 1, 0.0)), "{counted:?}");
        assert!(
            counted.contains(&("/cart".to_string(), 1, 0.0)),
            "{counted:?}"
        );

        let versions = run(&ctx, browser, "versions", "全部", "1 hour").await;

        assert_eq!(versions[0][0], json!("2.4.0"));
        assert_eq!(versions[0][1], json!(1));
        assert_eq!(versions[1][0], json!("2.4.1"));
        assert_eq!(versions[1][1], json!(2));

        let countries = run(&ctx, browser, "countries", "全部", "1 hour").await;
        assert_eq!(
            countries,
            vec![vec![json!("美国"), json!(2)], vec![json!("中国"), json!(1)]]
        );

        let browsers = run(&ctx, browser, "browsers", "全部", "1 hour").await;
        assert_eq!(
            browsers,
            vec![
                vec![json!("Chrome"), json!(2)],
                vec![json!("Safari"), json!(1)]
            ]
        );

        let elements = run(&ctx, browser, "frustrated-elements", "全部", "1 hour").await;
        assert_eq!(
            elements,
            vec![vec![json!("提交订单"), json!("/checkout"), json!(2)]]
        );

        let hosts = run(&ctx, browser, "slow-hosts", "全部", "1 hour").await;
        assert_eq!(
            hosts,
            vec![vec![json!("api.shop.example"), json!(400.0), json!(20.0)]]
        );
    }

    #[tokio::test]
    async fn a_chart_over_time_is_in_buckets_of_the_chosen_interval() {
        let ctx = context();
        let browser = RumDashboard::Browser;

        let hourly = run(&ctx, browser, "sessions-trend", "全部", "1 hour").await;
        assert_eq!(
            hourly,
            vec![vec![json!(T), json!(2)], vec![json!(2 * T), json!(1)]]
        );

        let quarters = run(&ctx, browser, "sessions-trend", "全部", "15 minutes").await;
        assert_eq!(quarters.len(), 2);

        let errors = run(&ctx, browser, "errors-trend", "全部", "1 hour").await;
        assert_eq!(errors, vec![vec![json!(T), json!(1), json!(2)]]);
    }

    fn measure(
        function: &str,
        field: Option<&str>,
        filters: Vec<Filter>,
        name: &str,
    ) -> MeasureSpec {
        MeasureSpec {
            function: function.into(),
            field: field.map(str::to_string),
            fallback: None,
            filters,
            name: Some(name.into()),
        }
    }

    fn condition(field: &str, op: &str, value: &str) -> Filter {
        Filter {
            field: field.into(),
            op: op.into(),
            value: value.into(),
            quoted: false,
        }
    }

    /// The figures the overview reads of a stream for an application and, if given, an environment:
    /// what `/rum/aggregate` computes, run on the same rows as the panels.
    async fn aggregate(
        ctx: &SessionContext,
        stream: RumStream,
        environment: Option<&str>,
        measures: &[MeasureSpec],
    ) -> std::collections::HashMap<String, f64> {
        let plan = Plan::new(
            stream,
            &AggregateSpec {
                measures,
                group_by: &[],
                bars: None,
                order_by: None,
                descending: None,
                limit: None,
            },
        )
        .unwrap();

        let mut conditions = vec![Condition::equals("application.id", "shop")];
        conditions.extend(environment.map(|name| Condition::equals("env", name)));

        let statement = plan.first_sql(&Scope {
            stream,
            from: 0,
            to: i64::MAX,
            conditions: &conditions,
        });

        let batches = ctx.sql(&statement).await.unwrap().collect().await.unwrap();
        let (columns, rows) = batches_to_json(&batches);
        let answer = plan.answer(&columns, &rows).unwrap();

        answer
            .measures
            .iter()
            .zip(&answer.rows[0].values)
            .filter_map(|(info, value)| Some((info.name.clone(), value.as_f64()?)))
            .collect()
    }

    fn close(left: f64, right: f64) -> bool {
        (left - right).abs() <= 1e-6 * left.abs().max(right.abs()).max(1.0)
    }

    // The figures of the first row of the dashboard are the figures the overview shows: the same
    // rows, counted at the newest revision, and the same percentile.
    #[tokio::test]
    async fn the_numbers_of_the_dashboard_are_those_of_the_aggregate() {
        let ctx = context();
        let browser = RumDashboard::Browser;

        for environment in [None, Some("prod"), Some("staging")] {
            let shown = environment.unwrap_or("全部");

            let views = aggregate(
                &ctx,
                RumStream::Views,
                environment,
                &[
                    measure("count", None, Vec::new(), "views"),
                    measure(
                        "count",
                        None,
                        vec![condition("view.error.count", ">", "0")],
                        "errored",
                    ),
                    measure(
                        "p75",
                        Some("view.performance.lcp.timestamp"),
                        Vec::new(),
                        "lcp",
                    ),
                    measure(
                        "p75",
                        Some("view.performance.inp.duration"),
                        Vec::new(),
                        "inp",
                    ),
                    measure("p75", Some("view.performance.cls.score"), Vec::new(), "cls"),
                ],
            )
            .await;

            let sessions = aggregate(
                &ctx,
                RumStream::Sessions,
                environment,
                &[
                    measure("count", None, Vec::new(), "sessions"),
                    measure(
                        "count",
                        None,
                        vec![condition("session.error.count", "=", "0")],
                        "clean",
                    ),
                    measure(
                        "count",
                        None,
                        vec![condition("session.frustration.count", ">", "0")],
                        "frustrated",
                    ),
                ],
            )
            .await;

            let panel = async |id: &str| number(&run(&ctx, browser, id, shown, "1 hour").await);
            let share = |part: f64, whole: f64| 100.0 * part / whole;

            assert_eq!(panel("sessions").await, sessions["sessions"], "{shown}");
            assert!(
                close(
                    panel("clean-sessions").await,
                    share(sessions["clean"], sessions["sessions"])
                ),
                "{shown}"
            );
            assert!(
                close(
                    panel("frustrated-sessions").await,
                    share(sessions["frustrated"], sessions["sessions"])
                ),
                "{shown}"
            );

            // The dashboard says seconds and milliseconds where the stream holds nanoseconds.
            assert!(close(panel("lcp").await, views["lcp"] / 1e9), "{shown}");
            assert!(close(panel("inp").await, views["inp"] / 1e6), "{shown}");
            assert!(close(panel("cls").await, views["cls"]), "{shown}");

            // The error rate of the pages table, over every page, is the share the aggregate counts.
            let pages = run(&ctx, browser, "pages", shown, "1 hour").await;
            let viewed: f64 = pages.iter().map(|row| row[1].as_f64().unwrap()).sum();
            let errored: f64 = pages
                .iter()
                .map(|row| row[1].as_f64().unwrap() * row[4].as_f64().unwrap() / 100.0)
                .sum();

            assert_eq!(viewed, views["views"], "{shown}");
            assert!(close(errored, views["errored"]), "{shown}");
        }
    }
}
