// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::*;
use crate::domain::pipeline::metric::OTHER_SERIES;

const MINUTE: i64 = 60_000_000;

fn spec(value: Value) -> MetricSpec {
    let spec: MetricSpec = serde_json::from_value(value).unwrap();
    spec.validate().unwrap();
    spec
}

fn org() -> Id {
    Id("org".into())
}

fn record(metrics: &GeneratedMetrics, spec: &MetricSpec, event: Value, at: i64) {
    let (org, pipeline) = (org(), Id("p".into()));
    let at = TimestampMicros(at);

    metrics.record(Counted::of(&org, &pipeline, "step", spec, &event, at, at));
}

fn samples(batches: Vec<MetricBatch>) -> Vec<RawEvent> {
    batches
        .into_iter()
        .flat_map(|batch| batch.samples)
        .collect()
}

#[test]
fn a_count_adds_up_each_interval_by_the_time_events_carry() {
    let metrics = GeneratedMetrics::with_base(0);
    let errors = spec(json!({
        "name": "app_errors", "type": "count", "stream": "app_metrics", "groupBy": ["service"],
    }));

    for (service, at) in [
        ("cart", 10 * MINUTE + 1),
        ("cart", 10 * MINUTE + 59_000_000),
        ("pay", 10 * MINUTE + 5),
    ] {
        record(&metrics, &errors, json!({ "service": service }), at);
    }

    record(&metrics, &errors, json!({ "service": "cart" }), 11 * MINUTE);

    // The interval waits for events still on their way.
    assert!(
        metrics
            .take_due(TimestampMicros(11 * MINUTE + 1))
            .is_empty()
    );

    let batches = metrics.take_due(TimestampMicros(11 * MINUTE + GRACE_MICROS));
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].stream, "app_metrics");

    let mut written = samples(batches);
    written.sort_by(|a, b| {
        a.fields["service"]
            .as_str()
            .cmp(&b.fields["service"].as_str())
    });

    assert_eq!(written.len(), 2);
    assert_eq!(written[0].fields["value"], json!(2.0));
    assert_eq!(written[0].fields["metric_name"], "app_errors");
    assert_eq!(written[0].fields["metric_kind"], "counter");
    assert_eq!(written[0].fields["metric_temporality"], "delta");
    assert_eq!(written[0].fields["metric_monotonic"], true);
    assert_eq!(
        written[0].fields["metric_start_time_unix_nano"],
        json!(10 * MINUTE * 1_000)
    );
    assert_eq!(written[0].timestamp, TimestampMicros(11 * MINUTE - 1));
    assert_eq!(written[1].fields["value"], json!(1.0));

    // The next interval is still open.
    assert!(!metrics.is_empty());
}

#[test]
fn a_late_event_is_written_as_another_part_of_its_interval() {
    let metrics = GeneratedMetrics::with_base(0);
    let errors = spec(json!({ "name": "app_errors", "type": "count", "stream": "app" }));

    record(&metrics, &errors, json!({}), MINUTE + 1);
    let first = samples(metrics.take_due(TimestampMicros(3 * MINUTE)));

    record(&metrics, &errors, json!({}), MINUTE + 2);
    record(&metrics, &errors, json!({}), MINUTE + 3);
    let second = samples(metrics.take_due(TimestampMicros(3 * MINUTE)));

    assert_eq!(first[0].fields["value"], json!(1.0));
    assert_eq!(second[0].fields["value"], json!(2.0));

    // PromQL keeps one sample per series and timestamp: the parts stand apart, within the interval.
    assert_ne!(first[0].timestamp, second[0].timestamp);
    assert!(second[0].timestamp.0 > MINUTE && second[0].timestamp.0 < 2 * MINUTE);
}

#[test]
fn two_nodes_write_the_same_interval_at_different_times() {
    let errors = spec(json!({ "name": "app_errors", "type": "count", "stream": "app" }));
    let one = GeneratedMetrics::with_base(17);
    let other = GeneratedMetrics::with_base(250_000);

    record(&one, &errors, json!({}), 1);
    record(&other, &errors, json!({}), 2);

    let one = samples(one.take_all(TimestampMicros(3)));
    let other = samples(other.take_all(TimestampMicros(3)));

    assert_ne!(one[0].timestamp, other[0].timestamp);
}

#[test]
fn a_histogram_is_stored_the_way_histogram_quantile_reads_it() {
    let metrics = GeneratedMetrics::with_base(0);
    let latency = spec(json!({
        "name": "latency_ms", "type": "histogram", "field": "duration_ms",
        "buckets": [10, 100, 2.5e3], "stream": "app",
    }));

    for duration in [5, 10, 50, 5000] {
        record(&metrics, &latency, json!({ "duration_ms": duration }), 1);
    }

    record(&metrics, &latency, json!({ "duration_ms": "slow" }), 1);

    let written = samples(metrics.take_all(TimestampMicros(1)));
    let value = |name: &str, le: Option<&str>| {
        written
            .iter()
            .find(|sample| {
                sample.fields["metric_name"] == name
                    && sample.fields.get("le").and_then(Value::as_str) == le
            })
            .map(|sample| sample.fields["value"].clone())
    };

    assert_eq!(value("latency_ms_bucket", Some("10")), Some(json!(2.0)));
    assert_eq!(value("latency_ms_bucket", Some("100")), Some(json!(3.0)));
    assert_eq!(value("latency_ms_bucket", Some("2500")), Some(json!(3.0)));
    assert_eq!(value("latency_ms_bucket", Some("+Inf")), Some(json!(4.0)));
    assert_eq!(value("latency_ms_count", None), Some(json!(4.0)));
    assert_eq!(value("latency_ms_sum", None), Some(json!(5065.0)));

    let sum = written
        .iter()
        .find(|sample| sample.fields["metric_name"] == "latency_ms_sum")
        .unwrap();
    assert_eq!(sum.fields["metric_monotonic"], false);
    assert_eq!(sum.fields["metric_kind"], "histogram_sum");
}

#[test]
fn a_gauge_holds_the_latest_value_at_its_own_time() {
    let metrics = GeneratedMetrics::with_base(0);
    let queue = spec(json!({
        "name": "queue_depth", "type": "gauge", "field": "depth", "stream": "app",
    }));

    record(&metrics, &queue, json!({ "depth": 7 }), 30_000_000);
    record(&metrics, &queue, json!({ "depth": 3 }), 10_000_000);

    let written = samples(metrics.take_all(TimestampMicros(MINUTE)));

    assert_eq!(written.len(), 1);
    assert_eq!(written[0].fields["value"], json!(7.0));
    assert_eq!(written[0].timestamp, TimestampMicros(30_000_000));
    assert_eq!(written[0].fields["metric_kind"], "gauge");
    assert!(!written[0].fields.contains_key("metric_temporality"));
}

#[test]
fn series_past_the_cap_share_one() {
    let metrics = GeneratedMetrics::with_base(0);
    let requests = spec(json!({
        "name": "requests", "type": "count", "stream": "app", "groupBy": ["user"],
    }));

    for user in 0..MAX_SERIES + 5 {
        record(&metrics, &requests, json!({ "user": user }), 1);
    }

    let written = samples(metrics.take_all(TimestampMicros(1)));
    let other: Vec<_> = written
        .iter()
        .filter(|sample| sample.fields["user"] == OTHER_SERIES)
        .collect();

    assert_eq!(written.len(), MAX_SERIES + 1);
    assert_eq!(other.len(), 1);
    assert_eq!(other[0].fields["value"], json!(5.0));
}

#[test]
fn events_too_far_from_now_are_not_counted() {
    let metrics = GeneratedMetrics::with_base(0);
    let errors = spec(json!({ "name": "errors", "type": "count", "stream": "app" }));
    let now = TimestampMicros(10 * MAX_AGE_MICROS);

    let (org, pipeline) = (org(), Id("p".into()));

    for at in [now.0 - MAX_AGE_MICROS - 1, now.0 + MAX_AHEAD_MICROS + 1] {
        let counted = Counted::of(
            &org,
            &pipeline,
            "step",
            &errors,
            &json!({}),
            TimestampMicros(at),
            now,
        );
        assert!(counted.is_none());
        metrics.record(counted);
    }

    assert!(metrics.is_empty());
}
