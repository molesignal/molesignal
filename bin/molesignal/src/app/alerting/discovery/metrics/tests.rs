// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::*;

fn instant(columns: &[&str], rows: Vec<Vec<Value>>) -> QueryResult {
    QueryResult {
        columns: columns.iter().map(|column| column.to_string()).collect(),
        rows,
        scanned_rows: 0,
        took_ms: 0,
        federation: None,
    }
}

#[test]
fn instant_rows_become_series_without_the_metric_name() {
    let series = series_of(&instant(
        &["value", "__name__", "host.name", "mountpoint"],
        vec![vec![
            json!(0.93),
            json!("system.filesystem.utilization"),
            json!("db-1"),
            json!("/data"),
        ]],
    ));

    assert_eq!(series.len(), 1);
    assert_eq!(series[0].1, 0.93);
    assert_eq!(
        series[0].0.get("host.name").map(String::as_str),
        Some("db-1")
    );
    assert!(!series[0].0.contains_key("__name__"));

    let full = detection(&SATURATION[0], &series[0]);
    assert_eq!(
        full.key,
        "metric_saturation|system.filesystem.utilization|db-1 /data"
    );
    assert_eq!(full.severity, Severity::Warning);
    assert_eq!(full.title, "Disk of db-1 /data is 93% full");
}

#[test]
fn restarts_name_the_container() {
    let labels: BTreeMap<String, String> = [
        ("namespace", "shop"),
        ("pod", "cart-7f9"),
        ("container", "cart"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value.to_string()))
    .collect();
    let found = detection(&RESTARTS[0], &(labels, 4.0));

    assert_eq!(found.subject, "shop/cart-7f9/cart");
    assert_eq!(found.severity, Severity::Error);
}

#[test]
fn cpu_that_jumped_within_the_hour_is_a_shift() {
    let core = |host: &str, idle: f64| -> Series {
        (
            [("instance".to_string(), host.to_string())]
                .into_iter()
                .collect(),
            idle,
        )
    };
    let now = vec![core("a", 0.05), core("a", 0.15), core("b", 0.5)];
    let before = vec![core("a", 0.7), core("a", 0.9), core("b", 0.55)];

    let shifts = cpu_shifts(&CPU_IDLE[1], &now, &before);

    assert_eq!(shifts.len(), 1);
    assert_eq!(shifts[0].subject, "a");
    assert_eq!(shifts[0].severity, Severity::Error);
    assert!((shifts[0].baseline.unwrap() - 0.2).abs() < 1e-9);
}
