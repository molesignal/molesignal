// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::*;
use crate::shared::{ids::Id, time::TimestampMicros};

#[test]
fn service_identity_uses_stable_fallbacks_and_preferred_environment() {
    let fallback = ServiceIdentity::new(None, Some("  "), None, None);
    assert_eq!(fallback.namespace, DEFAULT_SERVICE_NAMESPACE);
    assert_eq!(fallback.name, DEFAULT_SERVICE_NAME);
    assert_eq!(fallback.environment, DEFAULT_DEPLOYMENT_ENVIRONMENT);

    let preferred = ServiceIdentity::new(
        Some(" shop "),
        Some(" checkout "),
        Some(" prod "),
        Some("legacy"),
    );
    assert_eq!(
        preferred,
        ServiceIdentity {
            namespace: "shop".into(),
            name: "checkout".into(),
            environment: "prod".into(),
        }
    );
}

#[test]
fn protocol_error_classification_matches_apm_contract() {
    assert_eq!(
        ProtocolStatus {
            otel_status: "ERROR".into(),
            ..ProtocolStatus::default()
        }
        .outcome(),
        ApmOutcome::Error
    );
    assert_eq!(
        ProtocolStatus {
            http_status_code: Some(503),
            ..ProtocolStatus::default()
        }
        .outcome(),
        ApmOutcome::Error
    );
    assert_eq!(
        ProtocolStatus {
            rpc_status_code: Some(7),
            ..ProtocolStatus::default()
        }
        .outcome(),
        ApmOutcome::Error
    );
    assert_eq!(
        ProtocolStatus {
            http_status_code: Some(404),
            ..ProtocolStatus::default()
        }
        .outcome(),
        ApmOutcome::Success
    );
    assert_eq!(ProtocolStatus::default().outcome(), ApmOutcome::Unknown);
}

#[test]
fn persisted_owner_snapshot_json_is_backward_compatible() {
    let fixture = r#"{
      "org_id":"org-1",
      "owner_id":"node-a",
      "bucket_at":1000000,
      "snapshot_seq":7,
      "dimension":{
        "kind":"transaction",
        "service":{"namespace":"shop","name":"checkout","environment":"prod"},
        "version":"2.5.0",
        "transaction":{"name":"POST /checkout","kind":"http"}
      },
      "measurements":{
        "request_count":2,
        "error_count":1,
        "latency":{
          "schema_version":1,
          "counts":[0,2],
          "sum_time":3000,
          "min_time":1000,
          "max_time":2000
        },
        "exemplars":[]
      },
      "updated_at":2000000
    }"#;
    let snapshot: OwnerSnapshot = serde_json::from_str(fixture).expect("v1 fixture");
    assert_eq!(snapshot.schema_version, APM_PERSISTENCE_SCHEMA_VERSION);
    assert_eq!(snapshot.org_id, Id::from_string("org-1"));
    assert_eq!(snapshot.measurements.overflow_count, 0);
    assert!(matches!(
        snapshot.dimension,
        BucketDimension::Transaction { .. }
    ));

    let value = serde_json::to_value(snapshot).expect("serialize snapshot");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["dimension"]["kind"], "transaction");
    assert_eq!(value["updated_at"], TimestampMicros(2_000_000).0);
}

#[test]
fn span_kind_controls_service_and_dependency_contributions() {
    let mut fact = ApmSpanFact {
        schema_version: APM_FACT_SCHEMA_VERSION,
        org_id: Id::from_string("org-1"),
        service: ServiceIdentity::new(None, Some("api"), None, None),
        service_version: None,
        service_instance_id: None,
        instrumentation: InstrumentationMetadata::default(),
        trace_id: "trace".into(),
        span_id: "span".into(),
        parent_span_id: Some("parent".into()),
        event_time: TimestampMicros(1),
        duration: 10,
        span_kind: ApmSpanKind::Client,
        outcome: ApmOutcome::Success,
        transaction: None,
        dependency: None,
        error: None,
        exception: None,
        response_status: None,
        timed_out: false,
        retried: false,
        instance: None,
    };
    assert!(!fact.contributes_service_red());
    assert!(fact.contributes_dependency());

    fact.span_kind = ApmSpanKind::Unspecified;
    fact.parent_span_id = None;
    assert!(fact.contributes_service_red());
    assert!(!fact.contributes_dependency());
}

#[test]
fn merged_histogram_quantiles_use_all_bucket_counts() {
    let schema = HistogramSchema::v1();
    let mut first = LatencyHistogram::empty(&schema);
    for _ in 0..99 {
        first.observe(&schema, 1_000).expect("observe fast");
    }
    first.observe(&schema, 60_000_000).expect("observe slow");

    let mut second = LatencyHistogram::empty(&schema);
    for _ in 0..100 {
        second.observe(&schema, 2_000).expect("observe second");
    }

    // Taking max(P95(first), P95(second)) would report 2ms. Merging the
    // bucket populations first puts P50/P95/P99 in the 1–2ms bucket, because
    // only 99 of 200 observations are at or below 1ms, while still retaining
    // the 60s maximum. Within the bucket the rank is read off a straight line.
    first.merge(&second).expect("compatible histogram merge");
    assert_eq!(first.count(), 200);
    assert_eq!(first.quantile(&schema, 0.50).expect("p50"), Some(1_010));
    assert_eq!(first.quantile(&schema, 0.95).expect("p95"), Some(1_910));
    assert_eq!(first.quantile(&schema, 0.99).expect("p99"), Some(1_990));
    assert_eq!(
        first.quantile(&schema, 1.0).expect("p100"),
        Some(60_000_000)
    );
    assert_eq!(first.max_time, Some(60_000_000));
}

#[test]
fn logarithmic_layout_has_eight_buckets_to_a_doubling() {
    let schema = HistogramSchema::current();
    schema.validate().expect("current layout validates");

    assert_eq!(schema.version, APM_HISTOGRAM_SCHEMA_VERSION);
    assert_eq!(schema.upper_bounds_time.len(), 162);
    assert_eq!(schema.upper_bounds_time.first(), Some(&125));
    assert_eq!(schema.upper_bounds_time[160], 131_072_000);
    assert_eq!(schema.upper_bounds_time.last(), Some(&u64::MAX));

    // Every power of two of a millisecond is a bound, so the old buckets up to 16s line up with new ones.
    for bound in [1_000, 2_000, 64_000, 1_024_000, 16_384_000] {
        assert!(
            schema.upper_bounds_time.contains(&bound),
            "{bound} is a bound"
        );
    }

    assert_eq!(HistogramSchema::of_stored(2, 162), Some(schema));
    assert_eq!(
        HistogramSchema::of_stored(1, 18),
        Some(HistogramSchema::v1())
    );
    assert_eq!(
        HistogramSchema::of_stored(3, 82),
        Some(HistogramSchema::logarithmic(3, 4))
    );
    assert_eq!(HistogramSchema::of_stored(1, 162), None);
    assert_eq!(HistogramSchema::of_stored(2, 50), None);
}

#[test]
fn fine_buckets_read_percentiles_within_a_few_percent() {
    let schema = HistogramSchema::current();
    let mut histogram = LatencyHistogram::empty(&schema);

    // One request for every millisecond from 1ms to 1s.
    for millis in 1..=1_000_u64 {
        histogram.observe(&schema, millis * 1_000).expect("observe");
    }

    for (quantile, expected) in [(0.50, 500_000_f64), (0.95, 950_000.0), (0.99, 990_000.0)] {
        let read = histogram
            .quantile(&schema, quantile)
            .expect("quantile")
            .expect("value") as f64;
        assert!(
            (read - expected).abs() / expected < 0.04,
            "p{quantile}: {read} against {expected}"
        );
    }
}

#[test]
fn counts_the_observations_at_or_below_a_threshold_within_a_few_percent() {
    let schema = HistogramSchema::current();
    let mut histogram = LatencyHistogram::empty(&schema);

    for millis in 1..=1_000_u64 {
        histogram.observe(&schema, millis * 1_000).expect("observe");
    }

    for (threshold, expected) in [
        (500_000_u64, 500_f64),
        (100_000, 100.0),
        (2_000_000, 1_000.0),
    ] {
        let read = histogram
            .count_at_or_below(&schema, threshold)
            .expect("count");
        assert!(
            (read - expected).abs() / expected < 0.04,
            "at {threshold}: {read} against {expected}"
        );
    }

    assert_eq!(histogram.count_at_or_below(&schema, 0).expect("count"), 0.0);
    assert_eq!(
        LatencyHistogram::empty(&schema)
            .count_at_or_below(&schema, 1_000)
            .expect("count"),
        0.0
    );
}

#[test]
fn legacy_histograms_convert_without_losing_observations() {
    let legacy = HistogramSchema::v1();
    let current = HistogramSchema::current();
    let mut histogram = LatencyHistogram::empty(&legacy);

    for micros in [900, 1_500, 1_700, 300_000, 700_000, 45_000_000] {
        histogram.observe(&legacy, micros).expect("observe");
    }

    let converted = histogram.converted(&legacy, &current).expect("convert");
    assert_eq!(converted.schema_version, current.version);
    assert_eq!(converted.count(), histogram.count());
    assert_eq!(converted.sum_time, histogram.sum_time);
    assert_eq!(converted.max_time, Some(45_000_000));

    // The two 1–2ms observations stay between 1ms and 2ms.
    let between = current
        .upper_bounds_time
        .iter()
        .zip(&converted.counts)
        .filter(|(bound, _)| **bound > 1_000 && **bound <= 2_000)
        .map(|(_, count)| count)
        .sum::<u64>();
    assert_eq!(between, 2);

    let buckets = converted.filled_buckets(&current).expect("buckets");
    assert_eq!(buckets.iter().map(|bucket| bucket.count).sum::<u64>(), 6);
    // Each stays inside the old bucket it came from, narrowed to the least and greatest observation.
    let first = buckets.first().expect("first bucket");
    assert!(
        first.lower_time >= 900 && first.upper_time <= 1_000,
        "{first:?}"
    );
    let last = buckets.last().expect("last bucket");
    assert!(
        last.lower_time >= 30_000_000 && last.upper_time <= 45_000_000,
        "{last:?}"
    );
}

#[test]
fn histogram_merge_rejects_schema_mismatch() {
    let schema = HistogramSchema::v1();
    let mut left = LatencyHistogram::empty(&schema);
    let mut right = LatencyHistogram::empty(&schema);
    right.schema_version = right.schema_version.saturating_add(1);
    assert!(left.merge(&right).is_err());
}

#[test]
fn a_resolved_error_group_seen_again_has_regressed() {
    let issue = ErrorIssue {
        fingerprint: "fp".into(),
        status: ErrorIssueStatus::Resolved,
        assignee_id: None,
        team_id: None,
        resolved_at: Some(TimestampMicros(1_000)),
        updated_by: Id::from_string("u"),
        updated_at: TimestampMicros(1_000),
    };

    assert!(issue.regressed(TimestampMicros(2_000)));
    assert!(!issue.regressed(TimestampMicros(1_000)));

    let ignored = ErrorIssue {
        status: ErrorIssueStatus::Ignored,
        ..issue
    };

    assert!(!ignored.regressed(TimestampMicros(2_000)));
    assert_eq!(
        ErrorIssueStatus::parse("resolved"),
        Some(ErrorIssueStatus::Resolved)
    );
    assert_eq!(
        ErrorIssueStatus::parse("investigating"),
        Some(ErrorIssueStatus::Investigating)
    );
    assert_eq!(ErrorIssueStatus::parse("done"), None);

    let investigating = ErrorIssue {
        status: ErrorIssueStatus::Investigating,
        ..ignored
    };

    assert!(!investigating.regressed(TimestampMicros(2_000)));
}

#[test]
fn a_service_definition_needs_a_name_and_a_bounded_description() {
    let definition = ServiceDefinition {
        namespace: "shop".into(),
        name: "checkout".into(),
        description: "Takes orders.".into(),
        team_id: None,
        owner_id: None,
        tier: Some(ServiceTier::P0),
        lifecycle: Some(ServiceLifecycle::Production),
        repository_url: Some("https://github.com/acme/checkout".into()),
        tags: [("domain".to_owned(), "orders".to_owned())].into(),
        apdex_threshold: Some(300_000),
        key_transactions: Vec::new(),
        archived: false,
        updated_by: Id::from_string("u"),
        updated_at: TimestampMicros(0),
    };

    definition
        .validate()
        .expect("a described service validates");

    let git = ServiceDefinition {
        repository_url: Some("git@github.com:acme/checkout.git".into()),
        ..definition.clone()
    };

    assert!(git.validate().is_ok());

    // A password is masked before a definition is kept; one left in is refused.
    let unmasked = ServiceDefinition {
        repository_url: Some("https://ci:s3cr3t@git.example/acme/checkout.git".into()),
        ..definition.clone()
    };

    assert!(unmasked.validate().is_err());

    let nameless = ServiceDefinition {
        name: "  ".into(),
        ..definition.clone()
    };

    assert!(nameless.validate().is_err());

    let wordy = ServiceDefinition {
        description: "x".repeat(SERVICE_DESCRIPTION_MAX_LENGTH + 1),
        ..definition
    };

    assert!(wordy.validate().is_err());
    assert_eq!(ServiceTier::parse("p1"), Some(ServiceTier::P1));
    assert_eq!(ServiceTier::parse("critical"), None);
    assert_eq!(
        ServiceLifecycle::parse("deprecated").map(ServiceLifecycle::as_str),
        Some("deprecated")
    );
}

#[test]
fn a_service_definition_bounds_its_tags_apdex_and_key_transactions() {
    let base =
        ServiceDefinition::blank("shop", "checkout", Id::from_string("u"), TimestampMicros(0));
    let with = |change: &dyn Fn(&mut ServiceDefinition)| {
        let mut definition = base.clone();
        change(&mut definition);
        definition.validate()
    };

    assert!(
        with(&|definition| {
            definition
                .tags
                .insert("team/area".into(), "Payments EU".into());
            definition.tags.insert("pci".into(), String::new());
        })
        .is_ok()
    );
    assert!(
        with(&|definition| {
            definition.tags.insert("Team".into(), "x".into());
        })
        .is_err()
    );
    assert!(
        with(&|definition| {
            definition.tags.insert("team".into(), " padded".into());
        })
        .is_err()
    );
    assert!(
        with(&|definition| {
            definition.tags = (0..=SERVICE_TAGS_MAX)
                .map(|index| (format!("k{index}"), String::new()))
                .collect();
        })
        .is_err()
    );

    assert!(with(&|definition| definition.apdex_threshold = Some(0)).is_err());
    assert!(with(&|definition| definition.apdex_threshold = Some(61_000_000)).is_err());

    let pay = TransactionIdentity {
        name: "POST /pay".into(),
        kind: TransactionKind::Http,
    };
    let mut definition = base.clone();

    assert!(definition.mark_key_transaction(&pay, true));
    assert!(!definition.mark_key_transaction(&pay, true));
    assert_eq!(definition.key_transactions, std::slice::from_ref(&pay));
    assert!(definition.validate().is_ok());
    assert!(definition.mark_key_transaction(&pay, false));
    assert!(definition.key_transactions.is_empty());

    assert!(
        with(&|definition| definition.key_transactions = vec![TransactionIdentity::other()])
            .is_err()
    );
    assert!(
        with(&|definition| definition.key_transactions = vec![pay.clone(), pay.clone()]).is_err()
    );
}
