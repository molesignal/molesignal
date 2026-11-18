// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::{
    fields::Detected,
    probe::{Coverage, ValueCount},
    *,
};

fn detected(service: &str) -> Detected {
    Detected {
        service: Some(service.into()),
        environment: Some("env".into()),
        trace_id: Some("trace_id".into()),
        message: Some("message".into()),
        message_is_text: true,
        text_time: None,
        own_fields: 6,
    }
}

fn coverage(total: u64) -> Coverage {
    Coverage {
        total,
        service: total,
        environment: total,
        trace_id: total,
        json_messages: 0,
        time_sample: None,
    }
}

fn value(service: &str, environment: &str, count: u64) -> ValueCount {
    ValueCount {
        service: Some(service.into()),
        environment: Some(environment.into()),
        count,
    }
}

fn probe(name: &str, signal: Signal, field: &str, values: Vec<ValueCount>) -> Probe {
    Probe {
        name: name.into(),
        signal,
        detected: detected(field),
        coverage: Some(coverage(100)),
        values,
        error: None,
    }
}

fn kinds(found: &[Finding]) -> Vec<&str> {
    found.iter().map(|finding| finding.kind).collect()
}

#[test]
fn names_that_disagree_across_signals_and_environments_spelled_twice_are_found() {
    let probes = [
        probe(
            "app_logs",
            Signal::Logs,
            "service_name",
            vec![
                value("order-api", "prod", 80),
                value("payments", "production", 20),
            ],
        ),
        probe(
            "app_metrics",
            Signal::Metrics,
            "service_name",
            vec![value("order_api", "prod", 50)],
        ),
    ];

    let apm = [("order-api".to_string(), "prod".to_string())];
    let found = assess(&probes, &BTreeMap::new(), &apm);

    assert_eq!(
        kinds(&found),
        [
            "service_name_near_miss",
            "service_in_one_signal",
            "environment_spellings"
        ]
    );

    assert!(found[0].detail.contains("order-api (logs, traces)"));
    assert!(found[0].detail.contains("order_api (metrics)"));
    assert!(
        found[1]
            .detail
            .contains("Services seen only in logs: payments")
    );
    assert!(
        found[2]
            .detail
            .contains("prod (131 rows), production (20 rows)")
    );
}

#[test]
fn metric_names_without_units_and_streams_that_could_not_be_read_are_reported() {
    let mut unread = probe("slow_logs", Signal::Logs, "service_name", Vec::new());
    unread.coverage = None;
    unread.error = Some("the query took too long".into());

    let metrics = BTreeMap::from([
        ("rpc_latency".to_string(), None),
        ("rpc_latency_seconds".to_string(), None),
        ("jobs_done".to_string(), Some("counter".to_string())),
    ]);

    let found = assess(&[unread], &metrics, &[]);

    assert_eq!(
        kinds(&found),
        [
            "not_checked",
            "metric_without_unit",
            "counter_without_total"
        ]
    );

    assert!(
        found[0]
            .detail
            .contains("(the query took too long): slow_logs")
    );
}

#[test]
fn the_report_counts_by_severity_most_severe_first_and_sums_up_each_stream() {
    let mut blind = probe("raw", Signal::Logs, "service_name", Vec::new());
    blind.detected.service = None;

    let probes = [
        blind,
        probe(
            "app_logs",
            Signal::Logs,
            "app",
            vec![
                value("checkout", "prod", 10),
                value("checkout", "staging", 5),
                value("cart", "prod", 7),
            ],
        ),
    ];

    let found = assess(&probes, &BTreeMap::new(), &[]);
    let range = TimeRange::new(TimestampMicros(0), TimestampMicros(10));
    let report = report(range, &probes, found);

    assert_eq!(report["summary"]["critical"], 1);
    assert_eq!(report["findings"][0]["severity"], "critical");
    assert_eq!(report["findings"][0]["kind"], "no_service_field");
    assert_eq!(report["checkedStreams"], 2);

    let summary = &report["streams"][1];

    assert_eq!(summary["service"]["field"], "app");
    assert_eq!(summary["service"]["coverage"], 1.0);
    assert_eq!(summary["services"], json!(["checkout", "cart"]));
    assert_eq!(summary["traceId"]["field"], "trace_id");
}

#[test]
fn only_logs_traces_and_metrics_streams_are_checked() {
    assert_eq!(signal_of(StreamType::LOGS), Some(Signal::Logs));
    assert_eq!(signal_of(StreamType::METRICS), Some(Signal::Metrics));
    assert_eq!(signal_of(StreamType::TRACES), Some(Signal::Traces));
    assert_eq!(signal_of(StreamType::PROFILES), None);
}
