// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The report: the patterns of the window, each with its counts, when it was seen, its levels and
//! how it moved against the baseline; new and surging patterns first, patterns the window no
//! longer has last.

use std::{cmp::Reverse, collections::BTreeMap};

use serde::Serialize;

use super::{
    PatternQuery,
    change::{Change, classify, estimate},
    grouping::{Group, group_messages},
    stretch::{LogRow, Stretch},
    tokens::{Token, longest_fixed_run, pattern_key},
};

/// An example message is cut to this many characters.
pub(super) const EXAMPLE_CHARS: usize = 240;

/// The most levels told of one pattern.
const MAX_LEVELS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    pub start_time: i64,
    pub end_time: i64,
}

/// The stretch the window was compared with.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BaselineStretch {
    /// `previous` or `week_before`.
    pub kind: &'static str,
    pub start_time: i64,
    pub end_time: i64,
    pub logs: u64,
    pub read: usize,
    pub sampled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LevelCount {
    pub level: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pattern {
    /// The pattern as text, a changing place written as `<kind>`.
    pub pattern: String,
    /// Its words, when the query asked for them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tokens: Vec<Token>,
    /// How many of the window's logs read have it.
    pub count: usize,
    /// Its share of the window's logs read that say something, 0 to 1.
    pub share: f64,
    /// Its count over the whole window, when the window was sampled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_count: Option<f64>,
    pub first_seen: Option<i64>,
    pub last_seen: Option<i64>,
    pub levels: Vec<LevelCount>,
    /// How many of the baseline's logs read have it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change: Option<Change>,
    /// The window's estimated count over the baseline's; absent when the baseline had none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ratio: Option<f64>,
    /// The longest run of words every log of it has: what a query can ask for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fragment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatternReport {
    pub stream: String,
    pub field: String,
    pub level_field: Option<String>,
    pub time_range: Span,
    /// The window's logs that hold the message field.
    pub logs: u64,
    /// Those read.
    pub read: usize,
    /// Those read with nothing to say, in no pattern.
    pub unread: usize,
    pub sampled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline: Option<BaselineStretch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_patterns: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surging_patterns: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dropping_patterns: Option<usize>,
    pub pattern_count: usize,
    pub patterns: Vec<Pattern>,
}

/// One pattern and where it goes in the list.
struct Entry {
    rank: (u8, Reverse<usize>, Reverse<usize>, String),
    pattern: Pattern,
}

pub(super) fn report(
    query: &PatternQuery<'_>,
    current: &Stretch,
    baseline: Option<&Stretch>,
) -> PatternReport {
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
        .map(|group| entry(group, &rows, current, baseline, query.tokens))
        .collect::<Vec<_>>();

    entries.sort_by(|left, right| left.rank.cmp(&right.rank));

    let count_of = |wanted: Change| {
        baseline.map(|_| {
            entries
                .iter()
                .filter(|entry| entry.pattern.change == Some(wanted))
                .count()
        })
    };

    PatternReport {
        stream: query.stream.to_owned(),
        field: query.message.to_owned(),
        level_field: query.level.map(str::to_owned),
        time_range: Span {
            start_time: current.range.start.0,
            end_time: current.range.end.0,
        },
        logs: current.total,
        read: current.rows.len(),
        unread: current.rows.len() - current.said(),
        sampled: current.sampled(),
        baseline: baseline.map(|stretch| BaselineStretch {
            kind: query.baseline.as_str(),
            start_time: stretch.range.start.0,
            end_time: stretch.range.end.0,
            logs: stretch.total,
            read: stretch.rows.len(),
            sampled: stretch.sampled(),
        }),
        new_patterns: count_of(Change::New),
        surging_patterns: count_of(Change::Surging),
        dropping_patterns: count_of(Change::Dropping),
        pattern_count: entries.len(),
        patterns: entries
            .into_iter()
            .take(query.limit)
            .map(|entry| entry.pattern)
            .collect(),
    }
}

fn entry(
    group: &Group,
    rows: &[&LogRow],
    current: &Stretch,
    baseline: Option<&Stretch>,
    with_tokens: bool,
) -> Entry {
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
    let window_estimate = estimate(window_rows.len(), current.rows.len(), current.total);

    let (first_seen, last_seen) = told.iter().filter_map(|index| rows[*index].at).fold(
        (None, None),
        |(first, last): (Option<i64>, Option<i64>), at| {
            (
                Some(first.map_or(at, |first| first.min(at))),
                Some(last.map_or(at, |last| last.max(at))),
            )
        },
    );

    let (change, ratio) = match baseline {
        Some(stretch) => {
            let baseline_estimate = estimate(baseline_count, stretch.rows.len(), stretch.total);
            let (moved, ratio) = classify(window_estimate, baseline_estimate);
            (Some(moved), ratio.map(round4))
        }
        None => (None, None),
    };

    let fragment = longest_fixed_run(&group.tokens);

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
            key.clone(),
        ),
        pattern: Pattern {
            pattern: key,
            tokens: if with_tokens {
                group.tokens.clone()
            } else {
                Vec::new()
            },
            count: window_rows.len(),
            share: if said == 0 {
                0.0
            } else {
                round4(window_rows.len() as f64 / said as f64)
            },
            estimated_count: current.sampled().then(|| window_estimate.round()),
            first_seen,
            last_seen,
            levels: levels(told.iter().map(|index| rows[*index])),
            baseline_count: baseline.map(|_| baseline_count),
            change,
            ratio,
            fragment: (!fragment.is_empty()).then_some(fragment),
            example: told.first().map(|index| cut(&rows[*index].message)),
        },
    }
}

/// The levels of some logs, the most frequent first.
fn levels<'a>(rows: impl Iterator<Item = &'a LogRow>) -> Vec<LevelCount> {
    let mut counts = BTreeMap::<&str, usize>::new();

    for level in rows.filter_map(|row| row.level.as_deref()) {
        *counts.entry(level).or_default() += 1;
    }

    let mut counts = counts.into_iter().collect::<Vec<_>>();
    counts.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(right.0)));

    counts
        .into_iter()
        .take(MAX_LEVELS)
        .map(|(level, count)| LevelCount {
            level: level.to_owned(),
            count,
        })
        .collect()
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
