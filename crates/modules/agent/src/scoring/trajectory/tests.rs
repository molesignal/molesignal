// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::{Value, json};

use super::*;
use crate::runs::TranscriptCall;

const EMPTY_ROWS: &str = r#"[{"type":"json","json":{"columns":["n"],"rows":[]}}]"#;
const SOME_ROWS: &str = r#"[{"type":"json","json":{"columns":["n"],"rows":[[1],[2]]}}]"#;

fn call(name: &str, arguments: Value, result: &str) -> TranscriptCall {
    TranscriptCall {
        id: format!("c-{name}"),
        name: name.into(),
        arguments,
        result: Some(result.into()),
        truncated: false,
        is_error: false,
    }
}

fn failed(name: &str, arguments: Value) -> TranscriptCall {
    TranscriptCall {
        is_error: true,
        ..call(name, arguments, "query failed")
    }
}

fn many(name: &str, arguments: Value) -> TranscriptCall {
    TranscriptCall {
        truncated: true,
        ..call(
            name,
            arguments,
            r#"[{"type":"json","json":{"columns":["message"],"rows":[["a"],["b"]"#,
        )
    }
}

fn steps(calls: Vec<TranscriptCall>) -> Vec<TranscriptStep> {
    vec![TranscriptStep {
        text: String::new(),
        calls,
    }]
}

fn rules(trajectory: &Trajectory) -> Vec<GapRule> {
    trajectory.gaps.iter().map(|gap| gap.rule).collect()
}

#[test]
fn an_answer_without_tool_calls_has_no_trajectory() {
    assert_eq!(analyze(&[]), None);
    assert_eq!(analyze(&steps(Vec::new())), None);
}

#[test]
fn the_same_stream_queried_for_nothing_is_a_missing_data_map() {
    let logs = |sql: &str| json!({ "stream": "nginx", "sql": sql });
    let trajectory = analyze(&steps(vec![
        call(
            "query_logs",
            logs("SELECT count(*) FROM nginx WHERE app = 'x'"),
            EMPTY_ROWS,
        ),
        failed(
            "query_logs",
            logs("SELECT count(*) FROM nginx WHERE svc = 'x'"),
        ),
        call(
            "query_logs",
            logs("SELECT count(*) FROM nginx WHERE service = 'x'"),
            EMPTY_ROWS,
        ),
        call("query_logs", logs("SELECT count(*) FROM nginx"), SOME_ROWS),
    ]))
    .unwrap();

    assert_eq!(trajectory.calls, 4);
    assert_eq!(trajectory.empty_results, 2);
    assert_eq!(trajectory.failed_queries, 1);
    assert_eq!(trajectory.calls_before_first_data, Some(3));
    assert_eq!(
        trajectory.gaps,
        vec![Gap {
            kind: GapKind::DataMap,
            rule: GapRule::RepeatedQueries,
            stream: Some("nginx".into()),
            service: None,
            count: 4,
            misses: 3,
        }]
    );
}

#[test]
fn queries_that_mostly_find_data_are_no_gap() {
    let logs = json!({ "stream": "nginx", "sql": "SELECT count(*) FROM nginx" });
    let trajectory = analyze(&steps(vec![
        call("query_logs", logs.clone(), SOME_ROWS),
        call("query_logs", logs.clone(), EMPTY_ROWS),
        call("query_logs", logs, SOME_ROWS),
    ]))
    .unwrap();

    assert!(trajectory.gaps.is_empty());
    assert_eq!(trajectory.calls_before_first_data, Some(0));
}

#[test]
fn looking_up_the_structure_again_and_again_is_a_missing_data_map() {
    let trajectory = analyze(&steps(vec![
        call("list_streams", json!({}), SOME_ROWS),
        call(
            "get_stream_schema",
            json!({ "stream": "orders", "streamType": "logs" }),
            "{}",
        ),
        call(
            "get_stream_schema",
            json!({ "stream": "orders", "streamType": "logs" }),
            "{}",
        ),
        call("list_metric_names", json!({ "query": "order" }), "{}"),
    ]))
    .unwrap();

    assert_eq!(trajectory.schema_lookups, 4);
    assert_eq!(trajectory.calls_before_first_data, None);
    assert_eq!(rules(&trajectory), vec![GapRule::StructureLookups]);
    assert_eq!(trajectory.gaps[0].stream.as_deref(), Some("orders"));
    assert_eq!(trajectory.gaps[0].count, 4);
}

#[test]
fn a_service_with_metrics_but_no_logs_under_its_name_is_a_field_alignment_gap() {
    let trajectory = analyze(&steps(vec![
        call(
            "query_metrics",
            json!({ "promql": "rate(http_requests_total{service=\"order-api\"}[5m])" }),
            SOME_ROWS,
        ),
        call(
            "query_logs",
            json!({ "stream": "app_logs", "sql": "SELECT * FROM app_logs WHERE service = 'order-api'" }),
            EMPTY_ROWS,
        ),
    ]))
    .unwrap();

    assert_eq!(
        trajectory.gaps,
        vec![Gap {
            kind: GapKind::FieldAlignment,
            rule: GapRule::ServiceMismatch,
            stream: Some("app_logs".into()),
            service: Some("order-api".into()),
            count: 1,
            misses: 1,
        }]
    );
    assert!(trajectory.has(GapKind::FieldAlignment));
}

#[test]
fn a_service_found_in_the_logs_later_is_no_field_alignment_gap() {
    let logs = |sql: &str| json!({ "stream": "app_logs", "sql": sql });
    let trajectory = analyze(&steps(vec![
        call(
            "get_apm_service",
            json!({ "service": "order-api" }),
            r#"[{"type":"json","json":{"transactions":[{"name":"GET /orders"}]}}]"#,
        ),
        call(
            "query_logs",
            logs("SELECT * FROM app_logs WHERE service = 'order-api' AND level = 'error'"),
            EMPTY_ROWS,
        ),
        call(
            "query_logs",
            logs("SELECT * FROM app_logs WHERE service = 'order-api'"),
            SOME_ROWS,
        ),
    ]))
    .unwrap();

    assert!(!trajectory.has(GapKind::FieldAlignment));
}

#[test]
fn raw_log_pulls_without_patterns_need_a_feature_tool() {
    let pull = json!({ "stream": "nginx", "sql": "SELECT * FROM nginx" });
    let trajectory = analyze(&steps(vec![
        many("query_logs", pull.clone()),
        many(
            "search_around",
            json!({ "stream": "nginx", "anchorTime": 1 }),
        ),
        call(
            "query_logs",
            json!({ "stream": "nginx", "sql": "SELECT level, count(*) FROM nginx GROUP BY level" }),
            SOME_ROWS,
        ),
    ]))
    .unwrap();

    assert_eq!(trajectory.raw_log_pulls, 2);
    assert_eq!(rules(&trajectory), vec![GapRule::RawLogPulls]);
    assert_eq!(trajectory.gaps[0].stream.as_deref(), Some("nginx"));
    assert_eq!(trajectory.gaps[0].kind, GapKind::FeatureTools);

    let with_patterns = analyze(&steps(vec![
        many("query_logs", pull.clone()),
        many("query_logs", pull),
        call(
            "analyze_log_patterns",
            json!({ "stream": "nginx" }),
            r#"[{"type":"json","json":{"patterns":[{"pattern":"x"}]}}]"#,
        ),
    ]))
    .unwrap();

    assert!(with_patterns.gaps.is_empty());
    assert_eq!(
        with_patterns.feature_tools_used,
        vec!["analyze_log_patterns".to_string()]
    );
}

#[test]
fn raw_metric_pulls_without_a_trend_need_a_feature_tool() {
    let series = json!({ "promql": "rate(http_requests_total[5m])" });
    let trajectory = analyze(&steps(vec![
        many("query_metrics", series.clone()),
        many("query_metrics", series),
    ]))
    .unwrap();

    assert_eq!(rules(&trajectory), vec![GapRule::RawMetricPulls]);
    assert_eq!(
        trajectory.gaps[0].stream.as_deref(),
        Some("http_requests_total")
    );
    assert_eq!(
        trajectory.queries_by_stream,
        vec![StreamQueries {
            stream: "http_requests_total".into(),
            tool: "query_metrics".into(),
            count: 2,
            empty: 0,
            failed: 0,
        }]
    );
}

#[test]
fn the_trajectory_reads_back_from_its_json() {
    let trajectory = analyze(&steps(vec![
        call("list_streams", json!({}), SOME_ROWS),
        call("get_stream_schema", json!({ "stream": "orders" }), "{}"),
        call("get_stream_schema", json!({ "stream": "orders" }), "{}"),
    ]))
    .unwrap();

    let value = serde_json::to_value(&trajectory).unwrap();
    assert_eq!(value["gaps"][0]["rule"], "structure_lookups");
    assert_eq!(value["gaps"][0]["kind"], "data_map");
    assert_eq!(value["schemaLookups"], 3);

    let read: Trajectory = serde_json::from_value(value).unwrap();
    assert_eq!(read, trajectory);
}
