// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The report of `analyze_log_patterns`: the patterns of the window, each with its counts, when it
//! was seen, its levels and how it moved against the baseline; new and surging patterns first,
//! patterns the window no longer has last.

use std::{cmp::Reverse, collections::BTreeMap};

use serde_json::{Map, Value, json};

use super::{
    super::baseline::Baseline,
    LogRow, Stretch,
    change::{Change, classify, estimate},
    grouping::{Group, group_messages},
    tokens::{longest_fixed_run, pattern_key},
};

/// An example message is cut to this many characters.
pub(super) const EXAMPLE_CHARS: usize = 240;

/// The most levels told of one pattern.
const MAX_LEVELS: usize = 4;

/// What the report is about, besides the logs.
pub(super) struct Shape<'a> {
    pub(super) stream: &'a str,
    pub(super) field: &'a str,
    pub(super) level_field: Option<&'a str>,
    pub(super) baseline: Baseline,
    pub(super) limit: usize,
}

/// One pattern with what is told of it, and where it goes in the list.
struct Entry {
    rank: (u8, Reverse<usize>, Reverse<usize>, String),
    change: Option<Change>,
    value: Value,
}

pub(super) fn report(shape: &Shape<'_>, current: &Stretch, baseline: Option<&Stretch>) -> Value {
    // The window's logs first, then the baseline's: one reading of both.
    let rows = current
        .rows
        .iter()
        .chain(baseline.into_iter().flat_map(|stretch| stretch.rows.iter()))
        .collect::<Vec<_>>();

    let groups = group_messages(
        rows.iter()
            .enumerate()
            .filter(|(_, row)| row.says_something())
            .map(|(index, row)| (index, row.message.as_str())),
    );

    let mut entries = groups
        .iter()
        .map(|group| entry(group, &rows, current, baseline))
        .collect::<Vec<_>>();

    entries.sort_by(|left, right| left.rank.cmp(&right.rank));

    let count_of = |wanted: Change| {
        entries
            .iter()
            .filter(|entry| entry.change == Some(wanted))
            .count()
    };

    let mut out = Map::new();
    out.insert("stream".into(), json!(shape.stream));
    out.insert("field".into(), json!(shape.field));
    out.insert("levelField".into(), json!(shape.level_field));

    out.insert(
        "timeRange".into(),
        json!({"startTime": current.range.start.0, "endTime": current.range.end.0}),
    );

    out.insert("logs".into(), json!(current.total));
    out.insert("read".into(), json!(current.rows.len()));
    out.insert("unread".into(), json!(current.rows.len() - current.said()));
    out.insert("sampled".into(), json!(current.sampled()));

    if let Some(stretch) = baseline {
        out.insert(
            "baseline".into(),
            json!({
                "kind": shape.baseline.as_str(),
                "startTime": stretch.range.start.0,
                "endTime": stretch.range.end.0,
                "logs": stretch.total,
                "read": stretch.rows.len(),
                "sampled": stretch.sampled(),
            }),
        );

        out.insert("newPatterns".into(), json!(count_of(Change::New)));
        out.insert("surgingPatterns".into(), json!(count_of(Change::Surging)));
        out.insert("droppingPatterns".into(), json!(count_of(Change::Dropping)));
    }

    out.insert("patternCount".into(), json!(entries.len()));

    out.insert(
        "patterns".into(),
        Value::Array(
            entries
                .into_iter()
                .take(shape.limit)
                .map(|entry| entry.value)
                .collect(),
        ),
    );

    out.insert(
        "next".into(),
        json!(format!(
            "Read logs of a pattern with query_logs, filtering {} with LIKE '%<fragment>%', or \
             with search_around at its lastSeen time.",
            shape.field
        )),
    );

    Value::Object(out)
}

fn entry(group: &Group, rows: &[&LogRow], current: &Stretch, baseline: Option<&Stretch>) -> Entry {
    let in_window = |index: &&usize| **index < current.rows.len();

    let window_rows = group
        .rows
        .iter()
        .filter(in_window)
        .copied()
        .collect::<Vec<_>>();

    let baseline_count = group.rows.len() - window_rows.len();

    // What is told of a pattern is told from the window, or from the baseline when it is gone.
    let told = if window_rows.is_empty() {
        group.rows.clone()
    } else {
        window_rows.clone()
    };

    let key = pattern_key(&group.tokens);
    let said = current.said();
    let mut value = Map::new();

    value.insert("pattern".into(), json!(key));
    value.insert("count".into(), json!(window_rows.len()));

    value.insert(
        "share".into(),
        json!(if said == 0 {
            0.0
        } else {
            round4(window_rows.len() as f64 / said as f64)
        }),
    );

    let window_estimate = estimate(window_rows.len(), current.rows.len(), current.total);

    if current.sampled() {
        value.insert("estimatedCount".into(), json!(window_estimate.round()));
    }

    let moments = told.iter().filter_map(|index| rows[*index].at);

    let (first, last) = moments.fold(
        (None, None),
        |(first, last): (Option<i64>, Option<i64>), at| {
            (
                Some(first.map_or(at, |first| first.min(at))),
                Some(last.map_or(at, |last| last.max(at))),
            )
        },
    );

    value.insert("firstSeen".into(), json!(first));
    value.insert("lastSeen".into(), json!(last));

    value.insert(
        "levels".into(),
        levels(told.iter().map(|index| rows[*index])),
    );

    let mut change = None;

    if let Some(stretch) = baseline {
        let baseline_estimate = estimate(baseline_count, stretch.rows.len(), stretch.total);
        let (moved, ratio) = classify(window_estimate, baseline_estimate);

        value.insert("baselineCount".into(), json!(baseline_count));
        value.insert("change".into(), json!(moved));
        value.insert("ratio".into(), json!(ratio.map(round4)));
        change = Some(moved);
    }

    let fragment = longest_fixed_run(&group.tokens);

    if !fragment.is_empty() {
        value.insert("fragment".into(), json!(fragment));
    }

    if let Some(index) = told.first() {
        value.insert("example".into(), json!(cut(&rows[*index].message)));
    }

    // Gone patterns come last; then new and surging ones; then by how many logs.
    let rank_of_change = if window_rows.is_empty() {
        3
    } else {
        change.map_or(2, Change::rank)
    };

    Entry {
        rank: (
            rank_of_change,
            Reverse(window_rows.len()),
            Reverse(baseline_count),
            key,
        ),
        change,
        value: Value::Object(value),
    }
}

/// The levels of some logs, the most frequent first.
fn levels<'a>(rows: impl Iterator<Item = &'a LogRow>) -> Value {
    let mut counts = BTreeMap::<&str, usize>::new();

    for level in rows.filter_map(|row| row.level.as_deref()) {
        *counts.entry(level).or_default() += 1;
    }

    let mut counts = counts.into_iter().collect::<Vec<_>>();
    counts.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(right.0)));

    Value::Array(
        counts
            .into_iter()
            .take(MAX_LEVELS)
            .map(|(level, count)| json!({"level": level, "count": count}))
            .collect(),
    )
}

/// The first line of a message, cut to [`EXAMPLE_CHARS`] characters.
pub(super) fn cut(message: &str) -> String {
    let line = message.lines().next().unwrap_or_default().trim();

    if line.chars().count() <= EXAMPLE_CHARS {
        return line.to_string();
    }

    let mut text = line.chars().take(EXAMPLE_CHARS).collect::<String>();
    text.push('…');
    text
}

fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}
