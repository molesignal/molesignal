// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use domain::stream::{FieldDef, FieldType, StreamIndexType};
use serde_json::json;

use super::{
    summary::{EXAMPLE_CHARS, cut},
    *,
};

fn row(at: i64, message: &str, level: &str) -> LogRow {
    LogRow {
        at: Some(at),
        message: message.to_string(),
        level: (!level.is_empty()).then(|| level.to_string()),
    }
}

fn stretch(start: i64, rows: Vec<LogRow>, total: u64) -> Stretch {
    Stretch {
        range: TimeRange::new(
            TimestampMicros(start),
            TimestampMicros(start + 3_600_000_000),
        ),
        total,
        rows,
    }
}

fn shape(baseline: Baseline) -> Shape<'static> {
    Shape {
        stream: "app_logs",
        field: "message",
        level_field: Some("level"),
        baseline,
        limit: 30,
    }
}

fn schema(names: &[&str]) -> Schema {
    Schema {
        fields: names
            .iter()
            .map(|name| FieldDef {
                name: (*name).to_string(),
                data_type: FieldType::Utf8,
                nullable: true,
                index_type: StreamIndexType::None,
                encrypted: false,
            })
            .collect(),
    }
}

#[test]
fn the_message_and_level_fields_are_found_as_the_logs_page_finds_them() {
    let fields = schema(&["_timestamp", "Body", "log.level", "message"]);

    assert_eq!(find_field(&fields, MESSAGE_FIELDS), Some("message"));
    assert_eq!(find_field(&fields, LEVEL_FIELDS), Some("log.level"));

    // Named another way, a field is still found by its letters.
    let fields = schema(&["_timestamp", "Body", "LogLevel"]);

    assert_eq!(find_field(&fields, MESSAGE_FIELDS), Some("Body"));
    assert_eq!(find_field(&fields, LEVEL_FIELDS), Some("LogLevel"));
    assert_eq!(find_field(&schema(&["_timestamp"]), MESSAGE_FIELDS), None);
}

#[test]
fn a_big_stretch_is_read_in_equal_parts_that_cover_it() {
    let range = TimeRange::new(TimestampMicros(1_000), TimestampMicros(1_400));
    let parts = sample_parts(range, 4);

    assert_eq!(parts.len(), 4);
    assert_eq!(parts[0].start.0, 1_000);
    assert_eq!(parts[0].end.0, 1_099);
    assert_eq!(parts[1].start.0, 1_100);
    assert_eq!(parts[3].end.0, 1_400);

    for pair in parts.windows(2) {
        assert_eq!(pair[0].end.0 + 1, pair[1].start.0);
    }
}

#[test]
fn a_new_pattern_comes_before_the_steady_ones_with_its_counts_and_example() {
    let base = 10_000_000_000;

    let mut window = Vec::new();

    for index in 0..6 {
        window.push(row(
            base + 3_000_000_000 - index,
            &format!("upstream timeout after {} retries", index + 1),
            "ERROR",
        ));
    }

    for index in 0..8 {
        window.push(row(base + 1_000 + index, "order accepted", "INFO"));
    }

    window.push(row(base + 10, "   ", "INFO"));

    let earlier = (0..8)
        .map(|index| row(base - 1_000_000 - index, "order accepted", "INFO"))
        .collect::<Vec<_>>();

    let current = stretch(base, window, 15);
    let baseline = stretch(base - 3_600_000_001, earlier, 8);
    let report = report(&shape(Baseline::Previous), &current, Some(&baseline));

    assert_eq!(report["logs"], 15);
    assert_eq!(report["unread"], 1);
    assert_eq!(report["sampled"], false);
    assert_eq!(report["newPatterns"], 1);
    assert_eq!(report["patternCount"], 2);

    let first = &report["patterns"][0];

    assert_eq!(first["pattern"], "upstream timeout after <number> retries");
    assert_eq!(first["change"], "new");
    assert_eq!(first["count"], 6);
    assert_eq!(first["baselineCount"], 0);
    assert_eq!(first["levels"][0]["level"], "ERROR");
    assert_eq!(first["lastSeen"], base + 3_000_000_000);
    assert_eq!(first["firstSeen"], base + 3_000_000_000 - 5);
    assert_eq!(first["fragment"], "upstream timeout after");

    let second = &report["patterns"][1];

    assert_eq!(second["pattern"], "order accepted");
    assert_eq!(second["change"], "steady");
    assert_eq!(second["baselineCount"], 8);
    assert_eq!(second["ratio"], 1.0);
}

#[test]
fn a_pattern_the_window_no_longer_has_is_listed_last_as_dropping() {
    let base = 10_000_000_000;

    let window = (0..3)
        .map(|index| row(base + index, "cache warmed", "INFO"))
        .collect::<Vec<_>>();

    let earlier = (0..12)
        .map(|index| row(base - 10 - index, "disk almost full on /var", "WARN"))
        .chain((0..3).map(|index| row(base - 100 - index, "cache warmed", "INFO")))
        .collect::<Vec<_>>();

    let report = report(
        &shape(Baseline::Previous),
        &stretch(base, window, 3),
        Some(&stretch(base - 3_600_000_001, earlier, 15)),
    );

    let patterns = report["patterns"].as_array().unwrap();
    let last = patterns.last().unwrap();

    assert_eq!(last["pattern"], "disk almost full on /var");
    assert_eq!(last["count"], 0);
    assert_eq!(last["change"], "dropping");
    assert_eq!(report["droppingPatterns"], 1);
}

#[test]
fn a_sampled_window_says_so_and_scales_its_counts() {
    let base = 10_000_000_000;

    let window = (0..10)
        .map(|index| row(base + index, "heartbeat ok", ""))
        .collect::<Vec<_>>();

    let report = report(&shape(Baseline::Skip), &stretch(base, window, 1_000), None);

    assert_eq!(report["sampled"], true);
    assert!(report.get("baseline").is_none());

    let pattern = &report["patterns"][0];

    assert_eq!(pattern["count"], 10);
    assert_eq!(pattern["estimatedCount"], 1_000.0);
    assert!(pattern.get("change").is_none());
    assert_eq!(pattern["levels"], json!([]));
}

#[test]
fn an_example_is_the_first_line_cut_to_a_readable_length() {
    assert_eq!(cut("failed\n  at main"), "failed");

    let long = "x".repeat(EXAMPLE_CHARS + 10);
    let cut_text = cut(&long);

    assert_eq!(cut_text.chars().count(), EXAMPLE_CHARS + 1);
    assert!(cut_text.ends_with('…'));
}
