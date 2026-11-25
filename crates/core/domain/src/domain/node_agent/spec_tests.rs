// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{TaskKind, spec::validate_spec};

fn ok(kind: TaskKind, spec: serde_json::Value) {
    validate_spec(kind, &spec).unwrap_or_else(|error| panic!("{spec}: {error}"));
}

fn refused(kind: TaskKind, spec: serde_json::Value) {
    assert!(
        validate_spec(kind, &spec).is_err(),
        "{spec} should be refused"
    );
}

#[test]
fn tools_take_their_own_options_only() {
    ok(
        TaskKind::Tool,
        json!({"tool": "dropwatch", "filter": "tcp and port 80", "duration_secs": 60,
               "max_events_per_second": 100, "devices": ["eth0"]}),
    );
    ok(
        TaskKind::Tool,
        json!({"tool": "tcpshark", "mode": "retransmit", "enable_tlp": false, "duration_secs": 60}),
    );
    ok(
        TaskKind::Tool,
        json!({"tool": "iotracing", "duration_secs": 10, "devices": ["259:3"], "max_stack": 10}),
    );
    ok(
        TaskKind::Tool,
        json!({"tool": "perf", "duration_secs": 10, "container_id": "c86df4740fab", "frequency_hz": 99, "mode": "offcpu"}),
    );

    refused(TaskKind::Tool, json!({"tool": "sh", "duration_secs": 10}));
    refused(TaskKind::Tool, json!({"tool": "perf", "duration_secs": 0}));
    refused(
        TaskKind::Tool,
        json!({"tool": "perf", "duration_secs": 10, "filter": "tcp"}),
    );
    refused(
        TaskKind::Tool,
        json!({"tool": "perf", "duration_secs": 10, "pid": 1, "container_id": "c"}),
    );
    refused(
        TaskKind::Tool,
        json!({"tool": "perf", "duration_secs": 10, "mode": "gpu"}),
    );
    refused(
        TaskKind::Tool,
        json!({"tool": "tcpshark", "duration_secs": 10, "mode": "syn"}),
    );
    refused(
        TaskKind::Tool,
        json!({"tool": "tcpshark", "duration_secs": 10, "enable_tlp": "yes"}),
    );
    refused(
        TaskKind::Tool,
        json!({"tool": "iotracing", "duration_secs": 10, "devices": ["sda"]}),
    );
    refused(
        TaskKind::Tool,
        json!({"tool": "dropwatch", "duration_secs": 10, "devices": ["eth0"], "devices_excluded": ["lo"]}),
    );
}

#[test]
fn an_autotracing_run_names_one_scenario() {
    ok(TaskKind::AutotracingRun, json!({"rule": "memburst"}));
    refused(TaskKind::AutotracingRun, json!({"rule": "cpu"}));
    refused(
        TaskKind::AutotracingRun,
        json!({"rule": "dload", "extra": 1}),
    );
    refused(TaskKind::AutotracingRun, json!({}));
}

#[test]
fn a_profile_takes_a_window_target_frequency_and_mode() {
    ok(TaskKind::Profile, json!({"duration_secs": 30}));
    ok(
        TaskKind::Profile,
        json!({"duration_secs": 30, "pid": 42, "frequency_hz": 49, "mode": "physical_usage"}),
    );
    refused(
        TaskKind::Profile,
        json!({"duration_secs": 30, "tool": "perf"}),
    );
    refused(TaskKind::Profile, json!({"duration_secs": 4000}));
    refused(TaskKind::Profile, json!({"duration_secs": 30, "pid": 0}));
    refused(TaskKind::Profile, json!([]));

    ok(
        TaskKind::Profile,
        json!({"duration_secs": 30, "language": "java", "mode": "alloc"}),
    );
    ok(
        TaskKind::Profile,
        json!({"duration_secs": 30, "mode": "offcpu", "phase": "runqueue", "min_duration_us": 500}),
    );
    refused(
        TaskKind::Profile,
        json!({"duration_secs": 30, "language": "go"}),
    );
    refused(
        TaskKind::Profile,
        json!({"duration_secs": 30, "language": "python", "phase": "all"}),
    );
    refused(
        TaskKind::Profile,
        json!({"duration_secs": 30, "phase": "sleep"}),
    );
    ok(
        TaskKind::Tool,
        json!({"tool": "perf", "duration_secs": 10, "language": "python"}),
    );
}
