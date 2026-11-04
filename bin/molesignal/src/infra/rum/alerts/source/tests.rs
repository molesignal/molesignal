// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use parking_lot::Mutex;
use serde_json::{Value, json};

use super::*;
use crate::domain::rum::{
    RumAlertMetric, RumApplication, RumApplicationPatch, RumApplicationSettings, RumIssue,
    RumIssueChange, RumIssueEnvironment, RumIssueSighting,
};

const MINUTE: i64 = 60_000_000;

/// An engine that answers every statement with the same thing and remembers what it was asked.
#[derive(Default)]
struct Engine {
    asked: Mutex<Vec<QueryRequest>>,
    answer: Mutex<Option<Result<QueryResult>>>,
}

impl Engine {
    fn answering(result: QueryResult) -> Arc<Self> {
        let engine = Arc::new(Self::default());

        *engine.answer.lock() = Some(Ok(result));

        engine
    }

    fn failing(error: Error) -> Arc<Self> {
        let engine = Arc::new(Self::default());

        *engine.answer.lock() = Some(Err(error));

        engine
    }

    fn calls(&self) -> usize {
        self.asked.lock().len()
    }
}

#[async_trait]
impl QueryEngine for Engine {
    async fn execute(&self, req: QueryRequest) -> Result<QueryResult> {
        self.asked.lock().push(req);

        match self.answer.lock().as_ref() {
            Some(Ok(result)) => Ok(QueryResult {
                columns: result.columns.clone(),
                rows: result.rows.clone(),
                scanned_rows: result.scanned_rows,
                took_ms: result.took_ms,
                federation: None,
            }),
            Some(Err(error)) => Err(Error::forbidden(error.to_string())),
            None => Err(Error::internal("the engine was not told what to answer")),
        }
    }
}

struct Applications {
    platform: Option<RumPlatform>,
}

#[async_trait]
impl RumApplicationRepository for Applications {
    async fn create(&self, application: RumApplication) -> Result<RumApplication> {
        Ok(application)
    }

    async fn get(&self, org_id: &Id, id: &str) -> Result<RumApplication> {
        let Some(platform) = self.platform else {
            return Err(Error::not_found("no such application"));
        };

        Ok(RumApplication {
            org_id: org_id.clone(),
            id: id.to_string(),
            name: id.to_string(),
            platform,
            settings: RumApplicationSettings::default(),
            created_by: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        })
    }

    async fn list(&self, _org_id: &Id) -> Result<Vec<RumApplication>> {
        Ok(Vec::new())
    }

    async fn update(
        &self,
        org_id: &Id,
        id: &str,
        _patch: RumApplicationPatch,
    ) -> Result<RumApplication> {
        self.get(org_id, id).await
    }

    async fn delete(&self, _org_id: &Id, _id: &str) -> Result<()> {
        Ok(())
    }
}

/// What was asked of the issues: the application, the environment, and the range.
type Asked = (String, Option<String>, i64, i64);

/// Issues that answer the two questions a rule asks of them and remember who asked.
#[derive(Default)]
struct Issues {
    first: Vec<RumIssueMoment>,
    back: Vec<RumIssueMoment>,
    asked: Mutex<Vec<Asked>>,
}

#[async_trait]
impl RumIssueRepository for Issues {
    async fn get(&self, _org_id: &Id, _id: &str) -> Result<Option<RumIssue>> {
        Ok(None)
    }

    async fn get_many(&self, _org_id: &Id, _ids: &[String]) -> Result<Vec<RumIssue>> {
        Ok(Vec::new())
    }

    async fn open(&self, _org_id: &Id, _issues: &[RumIssue]) -> Result<Vec<RumIssue>> {
        Ok(Vec::new())
    }

    async fn record_sightings(&self, _org_id: &Id, _sightings: &[RumIssueSighting]) -> Result<()> {
        Ok(())
    }

    async fn record_environments(
        &self,
        _org_id: &Id,
        _environments: &[RumIssueEnvironment],
    ) -> Result<()> {
        Ok(())
    }

    async fn mark_regressed(
        &self,
        _org_id: &Id,
        _id: &str,
        _resolved_at: TimestampMicros,
        _at: TimestampMicros,
        _version: Option<&str>,
        _environment: Option<&str>,
    ) -> Result<Option<RumIssue>> {
        Ok(None)
    }

    async fn first_seen_between(
        &self,
        _org_id: &Id,
        application_id: &str,
        environment: Option<&str>,
        from: TimestampMicros,
        to: TimestampMicros,
        _limit: usize,
    ) -> Result<Vec<RumIssueMoment>> {
        self.asked.lock().push((
            application_id.to_string(),
            environment.map(str::to_string),
            from.0,
            to.0,
        ));

        Ok(self.first.clone())
    }

    async fn regressed_between(
        &self,
        _org_id: &Id,
        _application_id: &str,
        _environment: Option<&str>,
        _from: TimestampMicros,
        _to: TimestampMicros,
        _limit: usize,
    ) -> Result<Vec<RumIssueMoment>> {
        Ok(self.back.clone())
    }

    async fn update(
        &self,
        _org_id: &Id,
        _id: &str,
        _change: &RumIssueChange,
        _by: &Id,
        _at: TimestampMicros,
    ) -> Result<Option<RumIssue>> {
        Ok(None)
    }
}

fn alerts(engine: Arc<Engine>, platform: Option<RumPlatform>, issues: Issues) -> RumAlerts {
    RumAlerts::new(
        engine,
        Arc::new(Applications { platform }),
        Arc::new(issues),
    )
}

fn query(metric: RumAlertMetric) -> RumAlertQuery {
    RumAlertQuery {
        application_id: "shop-web".into(),
        environment: Some("prod".into()),
        page: None,
        version: None,
        metric,
        min_samples: None,
        custom_name: None,
        statistic: None,
        search: None,
    }
}

fn org() -> Id {
    Id::from_string("org")
}

/// A window that is still going on.
fn now_window() -> TimeRange {
    let now = TimestampMicros::now().0;

    TimeRange::new(TimestampMicros(now - 15 * MINUTE), TimestampMicros(now))
}

/// The answer of an aggregate with one row of values, in the order of the measures.
fn aggregate(values: Vec<Value>) -> QueryResult {
    QueryResult {
        columns: (0..values.len())
            .map(|index| format!("__m{index}"))
            .collect(),
        rows: vec![values],
        scanned_rows: 1,
        took_ms: 1,
        federation: None,
    }
}

/// The views of the read of the Views group: the views and how many of them are stored, the ones
/// with errors, then each vital and the rows it is made of.
fn views_of(total: u64, stored: u64, errored: u64) -> Vec<Value> {
    vec![
        json!(total),
        json!(stored),
        json!(errored),
        json!(3_000_000_000.0),
        json!(stored),
        Value::Null,
        json!(0),
        Value::Null,
        json!(0),
        Value::Null,
        json!(0),
    ]
}

/// The views of an application that keeps every session: what they stand for is what is stored.
fn views(total: u64, errored: u64) -> Vec<Value> {
    views_of(total, total, errored)
}

#[tokio::test]
async fn a_share_is_read_through_the_aggregate_of_the_pages() {
    let engine = Engine::answering(aggregate(views(200, 10)));
    let source = alerts(
        engine.clone(),
        Some(RumPlatform::Browser),
        Issues::default(),
    );
    let range = now_window();

    let reading = source
        .read(&org(), &query(RumAlertMetric::ErrorRate), range)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reading.value, 5.0);
    assert_eq!(reading.samples, 200);

    let asked = engine.asked.lock();

    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].stream.as_ref().unwrap().name, "rum_views");
    assert_eq!(asked[0].time_range, range);
    // A view is counted at its latest revision, in the application and environment of the rule.
    assert!(asked[0].statement.contains("WITH \"latest\""));
    assert!(
        asked[0]
            .statement
            .contains("\"application.id\" = 'shop-web'")
    );
    assert!(asked[0].statement.contains("\"env\" = 'prod'"));
}

#[tokio::test]
async fn a_percentile_waits_for_enough_views_that_had_a_value() {
    let engine = Engine::answering(aggregate(views(30, 0)));
    let source = alerts(engine, Some(RumPlatform::Browser), Issues::default());

    // 30 views had a value and the figure asks for 50 unless the rule says otherwise.
    assert!(
        source
            .read(&org(), &query(RumAlertMetric::LcpP75), now_window())
            .await
            .unwrap()
            .is_none()
    );

    let exactly = RumAlertQuery {
        min_samples: Some(30),
        custom_name: None,
        statistic: None,
        ..query(RumAlertMetric::LcpP75)
    };

    let reading = source
        .read(&org(), &exactly, now_window())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reading.value, 3_000.0);
    assert_eq!(reading.samples, 30);

    let more = RumAlertQuery {
        min_samples: Some(31),
        custom_name: None,
        statistic: None,
        ..query(RumAlertMetric::LcpP75)
    };

    assert!(
        source
            .read(&org(), &more, now_window())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn a_rule_waits_for_the_rows_that_are_stored_not_for_what_they_stand_for() {
    // 800 views are estimated from the 100 that were kept; 40 of them met an error.
    let engine = Engine::answering(aggregate(views_of(800, 100, 40)));
    let source = alerts(engine, Some(RumPlatform::Browser), Issues::default());

    let asks_for = |min_samples| RumAlertQuery {
        min_samples: Some(min_samples),
        custom_name: None,
        statistic: None,
        ..query(RumAlertMetric::ErrorRate)
    };

    // The rows behind the share are 100, however many views they stand for.
    assert!(
        source
            .read(&org(), &asks_for(101), now_window())
            .await
            .unwrap()
            .is_none()
    );

    let reading = source
        .read(&org(), &asks_for(100), now_window())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reading.value, 5.0);
    assert_eq!(reading.samples, 100);
}

#[tokio::test]
async fn the_reads_of_one_scope_and_window_are_shared() {
    let engine = Engine::answering(aggregate(views(200, 10)));
    let source = alerts(
        engine.clone(),
        Some(RumPlatform::Browser),
        Issues::default(),
    );
    let range = now_window();

    // Two figures of the same views, one read.
    source
        .read(&org(), &query(RumAlertMetric::ErrorRate), range)
        .await
        .unwrap();
    source
        .read(&org(), &query(RumAlertMetric::LcpP75), range)
        .await
        .unwrap();
    assert_eq!(engine.calls(), 1);

    // Another environment is another read.
    let staging = RumAlertQuery {
        environment: Some("staging".into()),
        ..query(RumAlertMetric::ErrorRate)
    };

    source.read(&org(), &staging, range).await.unwrap();
    assert_eq!(engine.calls(), 2);
}

#[tokio::test]
async fn a_window_that_ended_long_ago_is_cut_to_a_grid_and_shared() {
    let engine = Engine::answering(aggregate(views(200, 10)));
    let source = alerts(
        engine.clone(),
        Some(RumPlatform::Browser),
        Issues::default(),
    );

    let day_ago = TimestampMicros::now().0 - 24 * 60 * MINUTE;
    let end = day_ago - day_ago.rem_euclid(5 * MINUTE) + MINUTE;

    let window =
        |end: i64| TimeRange::new(TimestampMicros(end - 15 * MINUTE), TimestampMicros(end));

    source
        .read(&org(), &query(RumAlertMetric::ErrorRate), window(end))
        .await
        .unwrap();
    // A minute later: the same grid cell, so the same read.
    source
        .read(
            &org(),
            &query(RumAlertMetric::ErrorRate),
            window(end + MINUTE),
        )
        .await
        .unwrap();

    assert_eq!(engine.calls(), 1);

    let asked = engine.asked.lock();
    let range = asked[0].time_range;

    assert_eq!(range.end.0 % (5 * MINUTE), 0);
    assert_eq!(range.end.0 - range.start.0, 15 * MINUTE);
}

#[tokio::test]
async fn a_stream_nothing_was_written_to_has_no_rows_and_no_failure() {
    let engine = Engine::failing(Error::forbidden("stream not found: rum_errors"));
    let source = alerts(engine, Some(RumPlatform::Browser), Issues::default());

    // Nothing happened is a count of zero, so a rule can watch for it…
    let errors = source
        .read(&org(), &query(RumAlertMetric::ErrorCount), now_window())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(errors.value, 0.0);

    // …but a share of nothing is no value.
    let engine = Engine::failing(Error::forbidden("stream not found: rum_views"));
    let source = alerts(engine, Some(RumPlatform::Browser), Issues::default());

    assert!(
        source
            .read(&org(), &query(RumAlertMetric::ErrorRate), now_window())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn any_other_failure_is_a_failure() {
    let engine = Engine::failing(Error::invalid("syntax"));
    let source = alerts(engine, Some(RumPlatform::Browser), Issues::default());

    assert!(
        source
            .read(&org(), &query(RumAlertMetric::ErrorCount), now_window())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn an_application_that_is_gone_has_no_figures_and_no_series() {
    let engine = Engine::answering(aggregate(views(200, 10)));
    let source = alerts(engine.clone(), None, Issues::default());

    assert!(
        source
            .read(&org(), &query(RumAlertMetric::ErrorRate), now_window())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        source
            .samples(&org(), &query(RumAlertMetric::ErrorRate), now_window())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        source
            .series(
                &org(),
                &query(RumAlertMetric::ErrorRate),
                now_window(),
                MINUTE
            )
            .await
            .is_err()
    );
    assert_eq!(engine.calls(), 0);
}

#[tokio::test]
async fn new_issues_are_counted_from_the_issues_of_the_environment() {
    let moments = vec![
        RumIssueMoment {
            issue_id: "a".into(),
            at: TimestampMicros(1),
        },
        RumIssueMoment {
            issue_id: "b".into(),
            at: TimestampMicros(2),
        },
    ];
    let issues = Issues {
        first: moments,
        ..Issues::default()
    };
    let engine = Engine::answering(aggregate(vec![]));
    let source = alerts(engine.clone(), Some(RumPlatform::Browser), issues);
    let range = now_window();

    let reading = source
        .read(&org(), &query(RumAlertMetric::NewIssues), range)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reading.value, 2.0);
    assert_eq!(engine.calls(), 0);
}

#[tokio::test]
async fn a_series_has_a_point_for_every_bar_and_a_count_of_zero_where_nothing_happened() {
    // Only the second of three bars has errors.
    let engine = Engine::answering(QueryResult {
        columns: vec!["__bar".into(), "__m0".into()],
        rows: vec![vec![json!(1), json!(4)]],
        scanned_rows: 1,
        took_ms: 1,
        federation: None,
    });
    let source = alerts(engine, Some(RumPlatform::Browser), Issues::default());

    let from = TimestampMicros::now().0 - 3 * MINUTE;
    let range = TimeRange::new(TimestampMicros(from), TimestampMicros(from + 3 * MINUTE));

    let points = source
        .series(&org(), &query(RumAlertMetric::ErrorCount), range, MINUTE)
        .await
        .unwrap();

    assert_eq!(
        points
            .iter()
            .map(|point| (point.at.0 - from, point.reading.as_ref().map(|r| r.value)))
            .collect::<Vec<_>>(),
        vec![(0, Some(0.0)), (MINUTE, Some(4.0)), (2 * MINUTE, Some(0.0))]
    );
}

#[tokio::test]
async fn a_series_of_a_share_has_no_value_in_a_bar_without_rows() {
    let engine = Engine::answering(QueryResult {
        columns: [
            "__bar", "__m0", "__m1", "__m2", "__m3", "__m4", "__m5", "__m6", "__m7", "__m8",
            "__m9", "__m10",
        ]
        .map(String::from)
        .to_vec(),
        rows: vec![[json!(0)].into_iter().chain(views(200, 20)).collect()],
        scanned_rows: 1,
        took_ms: 1,
        federation: None,
    });
    let source = alerts(engine, Some(RumPlatform::Browser), Issues::default());

    let from = TimestampMicros::now().0 - 2 * MINUTE;
    let range = TimeRange::new(TimestampMicros(from), TimestampMicros(from + 2 * MINUTE));

    let points = source
        .series(&org(), &query(RumAlertMetric::ErrorRate), range, MINUTE)
        .await
        .unwrap();

    assert_eq!(points[0].reading.as_ref().map(|r| r.value), Some(10.0));
    assert!(points[1].reading.is_none());
}

#[tokio::test]
async fn the_sessions_an_incident_is_told_by_come_from_the_worst_views() {
    let engine = Engine::answering(QueryResult {
        columns: [
            "session.id",
            "view.performance.lcp.timestamp",
            "view.performance.lcp.target_selector",
            "__ts",
        ]
        .map(String::from)
        .to_vec(),
        rows: vec![
            vec![
                json!("s-1"),
                json!(7_200_000_000.0),
                json!("img.hero"),
                json!(10),
            ],
            vec![json!("s-2"), json!(6_400_000_000.0), Value::Null, json!(20)],
        ],
        scanned_rows: 2,
        took_ms: 1,
        federation: None,
    });
    let source = alerts(
        engine.clone(),
        Some(RumPlatform::Browser),
        Issues::default(),
    );

    let samples = source
        .samples(&org(), &query(RumAlertMetric::LcpP75), now_window())
        .await
        .unwrap();

    assert_eq!(samples.len(), 2);
    assert_eq!(samples[0].session_id, "s-1");
    assert_eq!(samples[0].value, Some(7_200.0));
    assert_eq!(samples[0].detail.as_deref(), Some("img.hero"));
    assert_eq!(samples[1].detail, None);

    // A figure that is not about sessions asks for none.
    assert!(
        source
            .samples(&org(), &query(RumAlertMetric::SessionCount), now_window())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(engine.calls(), 1);
}

fn answer(columns: &[&str], rows: Vec<Vec<Value>>) -> QueryResult {
    QueryResult {
        columns: columns.iter().map(|name| (*name).to_string()).collect(),
        rows,
        scanned_rows: 1,
        took_ms: 1,
        federation: None,
    }
}

#[tokio::test]
async fn the_worst_issue_is_the_group_that_ranks_first() {
    let engine = Engine::answering(answer(
        &["__g0", "__m0"],
        vec![
            vec![json!("issue-1"), json!(42)],
            vec![json!("issue-2"), json!(7)],
        ],
    ));
    let source = alerts(
        engine.clone(),
        Some(RumPlatform::Browser),
        Issues::default(),
    );

    let reading = source
        .read(&org(), &query(RumAlertMetric::IssueImpact), now_window())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reading.value, 42.0);

    let asked = engine.asked.lock();

    assert_eq!(asked[0].stream.as_ref().unwrap().name, "rum_errors");
    assert!(asked[0].statement.contains("\"error.issue_id\""));
    assert!(asked[0].statement.contains("COUNT(DISTINCT"));
    assert!(
        asked[0].statement.ends_with("LIMIT 1"),
        "{}",
        asked[0].statement
    );
}

#[tokio::test]
async fn no_issue_at_all_is_a_figure_of_none_touched() {
    let engine = Engine::answering(answer(&[], Vec::new()));
    let source = alerts(engine, Some(RumPlatform::Browser), Issues::default());

    let reading = source
        .read(&org(), &query(RumAlertMetric::IssueImpact), now_window())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reading.value, 0.0);
}

#[tokio::test]
async fn a_figure_the_application_measures_is_read_by_its_name_and_its_statistic() {
    let engine = Engine::answering(aggregate(vec![json!(2_000_000_000.0), json!(40)]));
    let source = alerts(
        engine.clone(),
        Some(RumPlatform::Browser),
        Issues::default(),
    );

    let vital = RumAlertQuery {
        custom_name: Some("checkout_ready".into()),
        statistic: Some(RumAlertStatistic::P95),
        ..query(RumAlertMetric::CustomVital)
    };

    let reading = source
        .read(&org(), &vital, now_window())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reading.value, 2_000.0);
    assert_eq!(reading.samples, 40);

    let asked = engine.asked.lock();

    assert_eq!(asked[0].stream.as_ref().unwrap().name, "rum_vitals");
    assert!(
        asked[0]
            .statement
            .contains("\"vital.name\" = 'checkout_ready'")
    );
    assert!(
        asked[0].statement.contains("0.95"),
        "{}",
        asked[0].statement
    );
}

#[tokio::test]
async fn a_figure_the_application_measures_waits_for_enough_rows_unless_it_is_a_count() {
    let engine = Engine::answering(aggregate(vec![json!(2_000_000_000.0), json!(10)]));
    let source = alerts(engine, Some(RumPlatform::Browser), Issues::default());

    let timing = RumAlertQuery {
        custom_name: Some("cart_ready".into()),
        ..query(RumAlertMetric::CustomTiming)
    };

    assert!(
        source
            .read(&org(), &timing, now_window())
            .await
            .unwrap()
            .is_none()
    );

    let engine = Engine::answering(aggregate(vec![json!(10), json!(10)]));
    let source = alerts(engine, Some(RumPlatform::Browser), Issues::default());

    let count = RumAlertQuery {
        statistic: Some(RumAlertStatistic::Count),
        ..timing
    };

    let reading = source
        .read(&org(), &count, now_window())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reading.value, 10.0);
}

fn searched(kind: &str, function: &str, field: Option<&str>) -> RumAlertQuery {
    RumAlertQuery {
        search: Some(RumAlertSearch {
            kind: kind.into(),
            statement: "resource.url_host = 'api.shop.example'".into(),
            filters: vec![crate::domain::rum::RumAlertFilter {
                field: "resource.url_host".into(),
                op: "=".into(),
                value: "api.shop.example".into(),
            }],
            free_text: Vec::new(),
            measure: crate::domain::rum::RumAlertMeasure {
                function: function.into(),
                field: field.map(str::to_string),
            },
        }),
        ..query(RumAlertMetric::Query)
    }
}

#[tokio::test]
async fn a_rule_from_the_explorer_reads_its_query_over_the_rows_of_its_type() {
    let engine = Engine::answering(aggregate(vec![json!(450_000_000.0), json!(60)]));
    let source = alerts(
        engine.clone(),
        Some(RumPlatform::Browser),
        Issues::default(),
    );

    let slow = searched("resources", "p95", Some("resource.duration"));

    let reading = source
        .read(&org(), &slow, now_window())
        .await
        .unwrap()
        .unwrap();

    // Nanoseconds are read as milliseconds, made of the 60 rows that had a duration.
    assert_eq!(reading.value, 450.0);
    assert_eq!(reading.samples, 60);

    let asked = engine.asked.lock();

    assert_eq!(asked[0].stream.as_ref().unwrap().name, "rum_resources");
    assert!(
        asked[0]
            .statement
            .contains("\"resource.url_host\" = 'api.shop.example'"),
        "{}",
        asked[0].statement
    );
    assert!(
        asked[0].statement.contains("0.95"),
        "{}",
        asked[0].statement
    );
    assert!(
        asked[0].statement.contains("\"session.type\" IS NULL"),
        "{}",
        asked[0].statement
    );
}

#[tokio::test]
async fn a_count_of_a_query_is_nought_where_nothing_matched_and_the_rest_wait_for_rows() {
    let engine = Engine::answering(aggregate(vec![Value::Null, json!(0)]));
    let source = alerts(engine, Some(RumPlatform::Browser), Issues::default());

    let counted = source
        .read(&org(), &searched("resources", "count", None), now_window())
        .await
        .unwrap()
        .unwrap();

    assert_eq!((counted.value, counted.samples), (0.0, 0));

    let engine = Engine::answering(aggregate(vec![json!(450_000_000.0), json!(12)]));
    let source = alerts(engine, Some(RumPlatform::Browser), Issues::default());

    assert!(
        source
            .read(
                &org(),
                &searched("resources", "p95", Some("resource.duration")),
                now_window()
            )
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn a_rule_from_the_explorer_is_told_by_the_sessions_of_the_rows_it_matched() {
    let engine = Engine::answering(QueryResult {
        columns: vec!["session.id".into(), "__ts".into()],
        rows: vec![
            vec![json!("s1"), json!(3)],
            vec![json!("s1"), json!(2)],
            vec![json!("s2"), json!(1)],
        ],
        scanned_rows: 3,
        took_ms: 1,
        federation: None,
    });
    let source = alerts(
        engine.clone(),
        Some(RumPlatform::Browser),
        Issues::default(),
    );

    let found = source
        .samples(&org(), &searched("resources", "count", None), now_window())
        .await
        .unwrap();

    assert_eq!(
        found
            .iter()
            .map(|sample| sample.session_id.as_str())
            .collect::<Vec<_>>(),
        ["s1", "s2"]
    );
    assert_eq!(
        engine.asked.lock()[0].stream.as_ref().unwrap().name,
        "rum_resources"
    );
}
