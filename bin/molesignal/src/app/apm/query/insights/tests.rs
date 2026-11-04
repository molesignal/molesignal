// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::{HashMap, HashSet};

use super::{
    detect::{ServiceFigures, Stretch, anomalies},
    model::{ChangeSource, InsightKind},
    suggest::{Served, suggestions},
    *,
};
use crate::{
    app::apm::ServiceHealth,
    domain::apm::{
        BucketMeasurements, ChangeKind, DependencyIdentity, DeploymentStatus, HealthLines,
        HistogramSchema, LatencyHistogram, TransactionIdentity, TransactionKind,
        VersionObservation,
    },
    shared::ids::Id,
};

const MINUTE: i64 = 60_000_000;

fn service(name: &str) -> ServiceIdentity {
    ServiceIdentity {
        namespace: "shop".into(),
        name: name.into(),
        environment: "prod".into(),
    }
}

fn red(requests: u64, error_rate: f64, p95_micros: u64) -> RedSummary {
    RedSummary {
        request_count: requests,
        error_count: (requests as f64 * error_rate).round() as u64,
        error_rate,
        p95_time: Some(p95_micros),
        ..RedSummary::default()
    }
}

fn stretch(red: &RedSummary) -> Stretch<'_> {
    Stretch {
        red,
        seconds: 3_600.0,
    }
}

fn point(minute: i64, red: RedSummary) -> RedTrendPoint {
    RedTrendPoint {
        bucket_at: TimestampMicros(minute * MINUTE),
        red,
    }
}

#[test]
fn a_move_against_the_hour_before_and_the_week_before_is_an_anomaly_a_weekly_rhythm_is_not() {
    let (failing, steady, busy, new) = (
        service("checkout"),
        service("search"),
        service("cart"),
        service("orders"),
    );
    let (failing_now, failing_before) = (red(1_000, 0.10, 100_000), red(1_000, 0.01, 100_000));
    // Twice the traffic of the hour before, as every Monday morning.
    let (busy_now, busy_before, busy_week) = (
        red(4_000, 0.0, 100_000),
        red(2_000, 0.0, 100_000),
        red(3_900, 0.0, 100_000),
    );
    let steady_now = red(1_000, 0.01, 100_000);
    // Nothing a week ago to tell it by: the hour before decides.
    let (new_now, new_before) = (red(1_000, 0.0, 900_000), red(1_000, 0.0, 200_000));

    let figures = [
        ServiceFigures {
            service: &failing,
            now: stretch(&failing_now),
            previous: Some(stretch(&failing_before)),
            last_week: Some(stretch(&failing_before)),
            trend: &[],
            bucket_seconds: 60.0,
        },
        ServiceFigures {
            service: &busy,
            now: stretch(&busy_now),
            previous: Some(stretch(&busy_before)),
            last_week: Some(stretch(&busy_week)),
            trend: &[],
            bucket_seconds: 60.0,
        },
        ServiceFigures {
            service: &steady,
            now: stretch(&steady_now),
            previous: Some(stretch(&steady_now)),
            last_week: None,
            trend: &[],
            bucket_seconds: 60.0,
        },
        ServiceFigures {
            service: &new,
            now: stretch(&new_now),
            previous: Some(stretch(&new_before)),
            last_week: None,
            trend: &[],
            bucket_seconds: 60.0,
        },
    ];

    let found = anomalies(&figures, &HealthLines::default());
    let named = found
        .iter()
        .map(|insight| {
            (
                insight.service.name.as_str(),
                insight.metric.unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(named, [("checkout", "error_rate"), ("orders", "p95")]);
    assert_eq!(found[0].severity, InsightSeverity::Critical);
    assert_eq!(found[0].last_week, Some(0.01));
    assert_eq!(found[1].severity, InsightSeverity::Warning);
    assert_eq!(found[1].last_week, None);
}

#[test]
fn a_move_begins_where_the_buckets_first_stay_past_the_line() {
    let checkout = service("checkout");
    let (now, before) = (red(500, 0.10, 100_000), red(500, 0.01, 100_000));
    let trend = [
        point(0, red(100, 0.01, 100_000)),
        // One bad minute that did not last.
        point(1, red(100, 0.09, 100_000)),
        point(2, red(100, 0.01, 100_000)),
        point(3, red(100, 0.12, 100_000)),
        point(4, red(100, 0.14, 100_000)),
    ];
    let figures = [ServiceFigures {
        service: &checkout,
        now: stretch(&now),
        previous: Some(stretch(&before)),
        last_week: None,
        trend: &trend,
        bucket_seconds: 60.0,
    }];

    let found = anomalies(&figures, &HealthLines::default());

    assert_eq!(found[0].since, Some(TimestampMicros(3 * MINUTE)));
}

fn measured(calls: u64, each_micros: u64, schema: &HistogramSchema) -> BucketMeasurements {
    let mut latency = LatencyHistogram::empty(schema);

    for _ in 0..calls {
        latency.observe(schema, each_micros).expect("observe");
    }

    let mut measurements = BucketMeasurements::empty(schema);
    measurements.request_count = calls;
    measurements.latency = latency;
    measurements
}

fn bucket(dimension: BucketDimension, measurements: BucketMeasurements) -> MergedBucket {
    MergedBucket {
        bucket_at: TimestampMicros(0),
        dimension,
        measurements,
    }
}

#[test]
fn suggestions_name_where_the_time_goes_a_call_in_a_loop_and_a_slow_statement() {
    let schema = HistogramSchema::logarithmic(2, 8);
    let checkout = service("checkout");
    let call = |operation: &str| DependencyIdentity {
        category: DependencyCategory::Database,
        target: "postgres orders".into(),
        operation: Some(operation.into()),
    };
    let loop_call = "SELECT * FROM order_items WHERE order_id = ?";
    let slow_call = "SELECT * FROM orders WHERE created_at > ?";
    let dependencies = [
        bucket(
            BucketDimension::Dependency {
                service: checkout.clone(),
                version: None,
                dependency: call(loop_call),
            },
            measured(2_400, 5_000, &schema),
        ),
        bucket(
            BucketDimension::Dependency {
                service: checkout.clone(),
                version: None,
                dependency: call(slow_call),
            },
            measured(30, 300_000, &schema),
        ),
    ];
    let endpoint = |name: &str| TransactionIdentity {
        name: name.into(),
        kind: TransactionKind::Http,
    };
    let transactions = [
        bucket(
            BucketDimension::Transaction {
                service: checkout.clone(),
                version: None,
                transaction: endpoint("POST /orders"),
            },
            measured(80, 220_000, &schema),
        ),
        bucket(
            BucketDimension::Transaction {
                service: checkout.clone(),
                version: None,
                transaction: endpoint("GET /health"),
            },
            measured(20, 1_000, &schema),
        ),
    ];
    let mut served_red = red(100, 0.0, 250_000);
    served_red.duration_sum = 20_000_000;
    let served = HashMap::from([(
        checkout.clone(),
        Served {
            red: &served_red,
            health: ServiceHealth::Healthy,
        },
    )]);

    let found = suggestions(&served, &transactions, &dependencies, &schema).expect("suggestions");
    let by_kind = |kind: InsightKind| {
        found
            .iter()
            .filter(|insight| insight.kind == kind)
            .collect::<Vec<_>>()
    };

    let slow_dependency = by_kind(InsightKind::SlowDependency);
    assert_eq!(slow_dependency.len(), 1);
    assert_eq!(
        slow_dependency[0]
            .dependency
            .as_ref()
            .map(|dependency| dependency.target.as_str()),
        Some("postgres orders")
    );
    assert_eq!(slow_dependency[0].severity, InsightSeverity::Info);

    let repeated = by_kind(InsightKind::RepeatedCall);
    assert_eq!(repeated[0].value, 24.0);
    assert_eq!(
        repeated[0]
            .dependency
            .as_ref()
            .and_then(|dependency| dependency.operation.as_deref()),
        Some(loop_call)
    );

    let slow = by_kind(InsightKind::SlowQuery);
    assert_eq!(slow.len(), 1);
    assert_eq!(
        slow[0]
            .dependency
            .as_ref()
            .and_then(|dependency| dependency.operation.as_deref()),
        Some(slow_call)
    );

    let hot = by_kind(InsightKind::HotEndpoint);
    assert_eq!(
        hot[0]
            .transaction
            .as_ref()
            .map(|transaction| transaction.name.as_str()),
        Some("POST /orders")
    );
    assert!(hot[0].value > 0.8);
}

fn event(kind: ChangeKind, service: &str, version: &str, minute: i64) -> DeploymentEvent {
    DeploymentEvent {
        id: Id::from_string(format!("{service}-{version}-{minute}")),
        kind,
        namespace: String::new(),
        service: service.into(),
        environment: "prod".into(),
        version: version.into(),
        status: DeploymentStatus::Succeeded,
        commit: None,
        repository_url: None,
        pull_request_url: None,
        author: None,
        ci_url: None,
        description: None,
        deployed_at: TimestampMicros(minute * MINUTE),
        created_by: Id::from_string("token-1"),
        created_at: TimestampMicros(minute * MINUTE),
    }
}

#[test]
fn the_changes_near_a_move_are_its_own_and_those_of_what_it_calls() {
    let checkout = service("checkout");
    let mut insight = Insight::new(
        InsightKind::Anomaly,
        InsightSeverity::Warning,
        checkout.clone(),
        0.1,
    );
    let reported = [
        event(ChangeKind::Deployment, "checkout", "2.0.0", 50),
        event(ChangeKind::Configuration, "payments", "", 55),
        event(ChangeKind::FeatureFlag, "search", "", 58),
        event(ChangeKind::Deployment, "checkout", "1.9.0", 0),
    ];
    let observed = [
        VersionObservation {
            org_id: Id::from_string("org-1"),
            service: checkout.clone(),
            version: "2.0.0".into(),
            first_seen_at: TimestampMicros(51 * MINUTE),
            last_seen_at: TimestampMicros(60 * MINUTE),
            observation_count: 10,
        },
        VersionObservation {
            org_id: Id::from_string("org-1"),
            service: checkout.clone(),
            version: "2.0.1-canary".into(),
            first_seen_at: TimestampMicros(59 * MINUTE),
            last_seen_at: TimestampMicros(60 * MINUTE),
            observation_count: 1,
        },
    ];

    changes::attach(
        &mut insight,
        TimestampMicros(60 * MINUTE),
        &reported,
        &observed,
        &HashSet::from(["payments".to_owned()]),
    );

    let named = insight
        .changes
        .iter()
        .map(|change| {
            (
                change.service.as_str(),
                change.version.as_str(),
                change.source,
                change.downstream,
            )
        })
        .collect::<Vec<_>>();

    // The nearest first; the deployment spans saw is not said again beside its report.
    assert_eq!(
        named,
        [
            ("checkout", "2.0.1-canary", ChangeSource::Observed, false),
            ("payments", "", ChangeSource::Reported, true),
            ("checkout", "2.0.0", ChangeSource::Reported, false),
        ]
    );
}
