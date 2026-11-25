// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::{Capabilities, Health, HealthState, NodeConfig};

#[test]
fn a_configuration_keeps_only_the_keys_it_sets() {
    let config = NodeConfig::parse(json!({
        "collectors": {"deny": ["oom"]},
        "profiling": {"oncpu": true, "frequency_hz": 49},
    }))
    .unwrap();

    assert_eq!(
        config.to_value(),
        json!({"collectors": {"deny": ["oom"]}, "profiling": {"oncpu": true, "frequency_hz": 49}})
    );

    assert_eq!(NodeConfig::parse(json!({})).unwrap().to_value(), json!({}));
}

#[test]
fn unknown_keys_bad_names_and_out_of_range_values_are_refused() {
    for document in [
        json!({"collector": {}}),
        json!({"collectors": {"allow": ["Bad Name"]}}),
        json!({"collectors": {"allow": ["oom"], "deny": ["oom"]}}),
        json!({"profiling": {"interval_secs": 0}}),
        json!({"profiling": {"frequency_hz": 5000}}),
        json!({"profiling": {"unknown": true}}),
        json!({"memory_reclaim_blocked_threshold_ns": 10}),
        json!({"dropwatch_filter": "x".repeat(2000)}),
        json!([]),
    ] {
        assert!(NodeConfig::parse(document.clone()).is_err(), "{document}");
    }
}

#[test]
fn a_node_row_before_its_first_heartbeat_reads_back() {
    // `health` and `capabilities` default to `{}` in the table; a node just enrolled has nothing else.
    let health: Health = serde_json::from_value(json!({})).unwrap();
    assert_eq!(health.state, HealthState::Unknown);
    assert!(health.messages.is_empty());

    let capabilities: Capabilities = serde_json::from_value(json!({})).unwrap();
    assert!(capabilities.task_kinds.is_empty());
}
