// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use domain::apm::{
    ChangeKind, DeploymentEvent, DeploymentStatus, ServiceIdentity, VersionObservation,
};
use kernel::{
    ids::Id,
    time::{TimeRange, TimestampMicros},
};
use postgres::repositories::annotations::Annotation;

use super::*;

const MINUTE: i64 = 60_000_000;

fn query(services: &[&str], kinds: &[ChangeFeedKind]) -> ChangeFeedQuery {
    ChangeFeedQuery {
        range: TimeRange::new(TimestampMicros(0), TimestampMicros(60 * MINUTE)),
        services: services.iter().map(|name| (*name).to_owned()).collect(),
        namespace: None,
        environment: None,
        kinds: kinds.to_vec(),
        dashboard_id: Some("dash-1".into()),
        stream: None,
        limit: 100,
    }
}

fn reported(
    id: &str,
    kind: ChangeKind,
    service: &str,
    version: &str,
    minute: i64,
) -> DeploymentEvent {
    DeploymentEvent {
        id: Id::from_string(id),
        kind,
        namespace: "shop".into(),
        service: service.into(),
        environment: "prod".into(),
        version: version.into(),
        status: DeploymentStatus::Succeeded,
        commit: Some("abc123".into()),
        repository_url: None,
        pull_request_url: None,
        author: Some("ada".into()),
        ci_url: None,
        description: None,
        deployed_at: TimestampMicros(minute * MINUTE),
        created_by: Id::from_string("token"),
        created_at: TimestampMicros(minute * MINUTE),
    }
}

fn observed(service: &str, version: &str, minute: i64) -> VersionObservation {
    VersionObservation {
        org_id: Id::from_string("org"),
        service: ServiceIdentity::new(Some("shop"), Some(service), Some("prod"), None),
        version: version.into(),
        first_seen_at: TimestampMicros(minute * MINUTE),
        last_seen_at: TimestampMicros(minute * MINUTE),
        observation_count: 1,
    }
}

fn annotation(id: &str, dashboard: Option<&str>, from: i64, to: i64) -> Annotation {
    Annotation {
        id: Id::from_string(id),
        org_id: Id::from_string("org"),
        title: "Load test".into(),
        description: None,
        tags: vec!["test".into()],
        time_start: TimestampMicros(from * MINUTE),
        time_end: TimestampMicros(to * MINUTE),
        dashboard_id: dashboard.map(Id::from_string),
        stream_name: None,
        created_by: Id::from_string("user-1"),
        created_at: TimestampMicros(0),
    }
}

#[test]
fn every_kind_comes_in_one_list_the_latest_first() {
    let sources = Sources {
        reported: vec![
            reported("d1", ChangeKind::Deployment, "checkout", "2.0.0", 30),
            reported("s1", ChangeKind::Scaling, "checkout", "", 40),
        ],
        annotations: vec![annotation("a1", None, 10, 20)],
        versions: vec![observed("cart", "1.1.0", 50)],
        rules: Vec::new(),
    };

    let entries = merge(sources, &query(&[], &[]), &ChangeFeedKind::ALL);

    assert_eq!(
        entries.iter().map(|entry| entry.kind).collect::<Vec<_>>(),
        vec![
            ChangeFeedKind::Version,
            ChangeFeedKind::Scaling,
            ChangeFeedKind::Deployment,
            ChangeFeedKind::Annotation,
        ]
    );
    assert_eq!(entries[2].commit.as_deref(), Some("abc123"));
    assert_eq!(entries[1].version, None);
    assert_eq!(entries[3].end, Some(TimestampMicros(20 * MINUTE)));
}

#[test]
fn a_version_a_pipeline_reported_is_told_once() {
    let sources = Sources {
        reported: vec![reported(
            "d1",
            ChangeKind::Deployment,
            "checkout",
            "2.0.0",
            30,
        )],
        versions: vec![
            observed("checkout", "2.0.0", 31),
            observed("checkout", "2.1.0", 45),
        ],
        ..Sources::default()
    };

    let entries = merge(sources, &query(&[], &[]), &ChangeFeedKind::ALL);

    assert_eq!(
        entries
            .iter()
            .map(|entry| (entry.kind, entry.version.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            (ChangeFeedKind::Version, Some("2.1.0")),
            (ChangeFeedKind::Deployment, Some("2.0.0")),
        ]
    );
}

#[test]
fn only_wanted_kinds_of_the_services_asked_for_are_kept() {
    let sources = Sources {
        reported: vec![
            reported("d1", ChangeKind::Deployment, "checkout", "2.0.0", 30),
            reported("d2", ChangeKind::Deployment, "search", "9.0.0", 31),
            reported("f1", ChangeKind::FeatureFlag, "checkout", "", 32),
        ],
        annotations: vec![annotation("a1", None, 10, 20)],
        ..Sources::default()
    };

    // The reader may read reported changes, not annotations.
    let wanted = [ChangeFeedKind::Deployment, ChangeFeedKind::FeatureFlag];
    let entries = merge(sources, &query(&["checkout"], &[]), &wanted);

    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["f1", "d1"]
    );
}

#[test]
fn annotations_of_another_dashboard_or_outside_the_range_are_left_out() {
    let sources = Sources {
        annotations: vec![
            annotation("mine", Some("dash-1"), 5, 6),
            annotation("other", Some("dash-2"), 5, 6),
            annotation("everywhere", None, -30, 1),
            annotation("before", None, -30, -10),
        ],
        ..Sources::default()
    };

    let entries = merge(sources, &query(&[], &[]), &ChangeFeedKind::ALL);

    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["mine", "everywhere"]
    );
}

#[test]
fn the_list_stops_at_its_limit() {
    let sources = Sources {
        reported: (0..10)
            .map(|minute| {
                reported(
                    &format!("d{minute}"),
                    ChangeKind::Deployment,
                    "checkout",
                    "1",
                    minute,
                )
            })
            .collect(),
        ..Sources::default()
    };
    let mut asked = query(&[], &[]);
    asked.limit = 3;

    let entries = merge(sources, &asked, &ChangeFeedKind::ALL);

    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["d9", "d8", "d7"]
    );
}

#[test]
fn kinds_are_named_as_they_are_written() {
    assert_eq!(
        ChangeFeedKind::parse("feature_flag"),
        Some(ChangeFeedKind::FeatureFlag)
    );
    assert_eq!(ChangeFeedKind::parse("rule"), Some(ChangeFeedKind::Rule));
    assert_eq!(ChangeFeedKind::parse("deploy"), None);
    assert!(ChangeFeedKind::Scaling.reported() && !ChangeFeedKind::Version.reported());
}
