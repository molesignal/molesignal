// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use kernel::time::TimestampMicros;

use super::*;

const STEP: i64 = 60_000_000;
const START: i64 = 1_800_000_000_000_000;

fn labels(pairs: &[(&str, &str)]) -> Labels {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

fn series(pairs: &[(&str, &str)], start: i64, values: &[f64]) -> Series {
    Series {
        labels: labels(pairs),
        points: values
            .iter()
            .enumerate()
            .map(|(index, value)| (start + index as i64 * STEP, *value))
            .collect(),
    }
}

fn reading(start: i64, series: Vec<Series>) -> Reading {
    let whole = sum_by_step(&series);

    Reading {
        range: TimeRange::new(TimestampMicros(start), TimestampMicros(start + 60 * STEP)),
        series,
        whole,
        truncated: false,
    }
}

fn ask(baseline: Baseline, split_by: &[String]) -> Ask<'_> {
    Ask {
        promql: "sum by (host) (rate(http_errors_total[5m]))",
        step: STEP,
        baseline,
        split_by,
        limit: 10,
    }
}

fn flat(level: f64, count: usize) -> Vec<f64> {
    vec![level; count]
}

fn stepped(before: f64, after: f64, at: usize, count: usize) -> Vec<f64> {
    (0..count)
        .map(|index| if index < at { before } else { after })
        .collect()
}

#[test]
fn a_range_result_is_read_into_series_in_time_order() {
    let result = QueryResult {
        columns: vec!["_timestamp".into(), "value".into(), "host".into()],
        rows: vec![
            vec![json!(2), json!(1.5), json!("a")],
            vec![json!(1), json!(1.0), json!("a")],
            vec![json!(1), json!(3), json!("b")],
            vec![json!(2), Value::Null, json!("b")],
        ],
        scanned_rows: 4,
        took_ms: 0,
        federation: None,
    };

    let found = read_series(&result);

    assert_eq!(found.len(), 2);
    assert_eq!(found[0].labels["host"], "a");
    assert_eq!(found[0].points, [(1, 1.0), (2, 1.5)]);
    assert_eq!(found[1].points, [(1, 3.0)]);
    assert_eq!(sum_by_step(&found), [(1, 4.0), (2, 1.5)]);
}

#[test]
fn the_step_is_never_finer_than_the_window_allows() {
    let hour = TimeRange::new(TimestampMicros(0), TimestampMicros(3_600_000_000));

    assert_eq!(step_for(hour, None), 36_000_000);
    assert_eq!(step_for(hour, Some(1)), 7_200_000);
    assert_eq!(step_for(hour, Some(120)), 120_000_000);
}

#[test]
fn a_step_in_one_series_is_found_and_carried_by_that_series() {
    let window = reading(
        START,
        vec![
            series(&[("host", "a")], START, &stepped(1.0, 9.0, 30, 60)),
            series(&[("host", "b")], START, &flat(1.0, 60)),
        ],
    );

    let earlier = START - 60 * STEP - 1;

    let baseline = reading(
        earlier,
        vec![
            series(&[("host", "a")], earlier, &flat(1.0, 60)),
            series(&[("host", "b")], earlier, &flat(1.0, 60)),
        ],
    );

    let report = report(&ask(Baseline::Previous, &[]), &window, Some(&baseline));

    assert_eq!(report["whole"]["aggregate"], "sum");
    assert_eq!(report["changePoint"]["significant"], true);
    assert_eq!(report["changePoint"]["at"], START + 30 * STEP);
    assert_eq!(report["changePoint"]["direction"], "up");
    assert_eq!(report["baseline"]["moved"], true);
    assert_eq!(report["baseline"]["direction"], "up");

    let contributions = &report["contributions"];

    assert_eq!(contributions["comparedWith"], "baseline");
    assert_eq!(contributions["series"][0]["labels"]["host"], "a");
    assert_eq!(contributions["series"][0]["share"], 1.0);
    assert_eq!(contributions["byLabel"][0]["label"], "host");
    assert_eq!(contributions["byLabel"][0]["values"][0]["value"], "a");
}

#[test]
fn without_a_baseline_the_move_is_split_at_the_change_point() {
    let window = reading(
        START,
        vec![
            series(&[("route", "/pay")], START, &stepped(5.0, 1.0, 20, 60)),
            series(&[("route", "/cart")], START, &flat(2.0, 60)),
        ],
    );

    let report = report(&ask(Baseline::Skip, &[]), &window, None);

    assert!(report.get("baseline").is_none());
    assert_eq!(report["changePoint"]["direction"], "down");
    assert_eq!(report["contributions"]["comparedWith"], "before_change");
    assert_eq!(
        report["contributions"]["series"][0]["labels"]["route"],
        "/pay"
    );
    assert_eq!(report["contributions"]["series"][0]["delta"], -4.0);
}

#[test]
fn a_steady_window_has_no_significant_change_and_nothing_to_attribute() {
    let values = (0..60)
        .map(|index| 10.0 + [0.0, 0.5, -0.5][index % 3])
        .collect::<Vec<_>>();

    let window = reading(START, vec![series(&[], START, &values)]);
    let report = report(&ask(Baseline::Skip, &[]), &window, None);

    assert_eq!(report["whole"]["aggregate"], "series");
    assert_eq!(report["changePoint"]["significant"], false);
    assert!(report["contributions"]["note"].is_string());
}

#[test]
fn labels_asked_for_that_no_series_has_are_named() {
    let window = reading(START, vec![series(&[("host", "a")], START, &flat(2.0, 60))]);

    let earlier = START - 60 * STEP - 1;
    let baseline = reading(
        earlier,
        vec![series(&[("host", "a")], earlier, &flat(1.0, 60))],
    );
    let split_by = ["host".to_string(), "zone".to_string()];
    let report = report(
        &ask(Baseline::Previous, &split_by),
        &window,
        Some(&baseline),
    );

    assert_eq!(report["contributions"]["unknownLabels"], json!(["zone"]));
    assert_eq!(report["contributions"]["byLabel"][0]["label"], "host");
}

#[test]
fn a_window_without_data_says_so() {
    let report = report(
        &ask(Baseline::Previous, &[]),
        &reading(START, Vec::new()),
        None,
    );

    assert_eq!(report["seriesCount"], 0);
    assert!(report["note"].is_string());
    assert!(report.get("changePoint").is_none());
}
