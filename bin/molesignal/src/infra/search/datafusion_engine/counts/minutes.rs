// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Counting from segments' minute counts (see [`MinuteCounts`]). A file whose counts are kept by
//! every field a statement names is answered from them, unread, where the query window cuts it
//! only between minutes: a count with equality conditions, a count by one of those fields and a
//! count by bars a whole number of minutes wide.

use std::collections::{BTreeMap, HashMap};

use domain::{
    storage::QueryFile,
    stream::{FieldType, StreamDefinition},
};
use kernel::time::{TimeRange, TimestampMicros};
use query_language::BarKey;

use super::DataFusionEngine;
use crate::infra::{
    search::tantivy_index::{BarQuery, BarTally},
    storage::minute_counts::{MINUTE, MinuteCounts, MinuteCountsKeys, whole_minutes},
};

/// Equality conditions minute counts can answer: each on a text field kept in plain text, whose
/// values the counts hold as they are. Whether a file's counts are kept by the field is seen file
/// by file.
pub(super) fn answerable(
    filters: &[(String, String)],
    definition: Option<&StreamDefinition>,
) -> bool {
    filters
        .iter()
        .all(|(column, _)| definition.is_some_and(|definition| text_field(column, definition)))
}

pub(super) fn text_field(column: &str, definition: &StreamDefinition) -> bool {
    definition
        .schema
        .fields
        .iter()
        .any(|field| field.name == column && !field.encrypted && field.data_type == FieldType::Utf8)
}

/// Bars that minute counts can fill: a whole number of minutes wide, starting at a whole minute,
/// and none before `origin` inside the window, where truncating division would put the rows of
/// one minute in two bars.
pub(super) fn whole_minute_bars(origin: i64, width: i64, start: i64) -> bool {
    origin % MINUTE == 0 && width % MINUTE == 0 && start >= origin
}

/// Adds to `rows` the rows inside `window` meeting `filters` of every file its minute counts
/// answer for; returns the other files.
pub(super) async fn count_rows(
    engine: &DataFusionEngine,
    files: Vec<QueryFile>,
    keys: &HashMap<String, MinuteCountsKeys>,
    window: TimeRange,
    filters: &[(String, String)],
    rows: &mut u64,
) -> Vec<QueryFile> {
    answer(engine, files, keys, |file, counts| {
        let minutes = file_minutes(&window, file)?;
        let equal = counts.places(filters)?;

        *rows += counts
            .entries(minutes, &equal)
            .map(|entry| counts.rows(entry))
            .sum::<u64>();

        Some(())
    })
    .await
}

/// Adds to `counts` the rows inside `window` meeting `filters` of each value of `column`, and to
/// `nulls` those without a value, of every file its minute counts answer for; returns the other
/// files.
#[allow(clippy::too_many_arguments)]
pub(super) async fn count_groups(
    engine: &DataFusionEngine,
    files: Vec<QueryFile>,
    keys: &HashMap<String, MinuteCountsKeys>,
    window: TimeRange,
    column: &str,
    filters: &[(String, String)],
    counts: &mut HashMap<String, u64>,
    nulls: &mut u64,
) -> Vec<QueryFile> {
    answer(engine, files, keys, |file, minute_counts| {
        let minutes = file_minutes(&window, file)?;
        let equal = minute_counts.places(filters)?;
        let field = minute_counts.field(column)?;

        for entry in minute_counts.entries(minutes, &equal) {
            let rows = minute_counts.rows(entry);

            match minute_counts.value(field, entry) {
                Some(value) => *counts.entry(value.to_owned()).or_default() += rows,
                None => *nulls += rows,
            }
        }

        Some(())
    })
    .await
}

/// Adds to `bars` the rows of `query` of every file its minute counts answer for, each bar named
/// as `key` says; returns the other files. The bars are whole minutes, see
/// [`whole_minute_bars`].
pub(super) async fn count_bars(
    engine: &DataFusionEngine,
    files: Vec<QueryFile>,
    keys: &HashMap<String, MinuteCountsKeys>,
    query: &BarQuery,
    key: BarKey,
    bars: &mut BTreeMap<i64, BarTally>,
) -> Vec<QueryFile> {
    let window = TimeRange::new(
        TimestampMicros(query.window.start),
        TimestampMicros(query.window.end),
    );

    answer(engine, files, keys, |file, counts| {
        let minutes = file_minutes(&window, file)?;
        let equal = counts.places(&query.terms)?;

        let errors = match &query.errors {
            Some(level) => Some(counts.field(level)?),
            None => None,
        };

        for entry in counts.entries(minutes, &equal) {
            let rows = counts.rows(entry);
            let start = counts.minute(entry).saturating_mul(MINUTE);
            let index = start.saturating_sub(query.origin) / query.width;

            let bar = match key {
                BarKey::Index => index,
                BarKey::Start => index * query.width,
            };

            let errors = errors
                .and_then(|field| counts.value(field, entry))
                .is_some_and(|level| level.to_uppercase() == "ERROR");

            let tally = bars.entry(bar).or_default();
            tally.total += rows;
            tally.errors += if errors { rows } else { 0 };
        }

        Some(())
    })
    .await
}

/// Reads the minute counts of `files` and lets `count` answer for each file that has them;
/// returns the files without counts and those `count` could not answer for. Counts that do not
/// add up to their file's rows are not used.
async fn answer(
    engine: &DataFusionEngine,
    files: Vec<QueryFile>,
    keys: &HashMap<String, MinuteCountsKeys>,
    mut count: impl FnMut(&QueryFile, &MinuteCounts) -> Option<()>,
) -> Vec<QueryFile> {
    if keys.is_empty() || files.is_empty() {
        return files;
    }

    let wanted = files
        .iter()
        .map(|file| {
            keys.get(file.id.as_str())
                .map(|keys| (file.id.as_str().to_owned(), keys.clone()))
        })
        .collect::<Vec<_>>();

    let loaded = engine
        .minute_counts
        .get_many(engine.object_store.as_ref(), wanted)
        .await;

    let mut rest = Vec::new();

    for (file, counts) in files.into_iter().zip(loaded) {
        let answered = counts
            .filter(|counts| counts.total() == file.rows)
            .and_then(|counts| count(&file, &counts));

        if answered.is_none() {
            rest.push(file);
        }
    }

    rest
}

/// The minutes of `file` inside `window`, when the window cuts it only between minutes.
fn file_minutes(window: &TimeRange, file: &QueryFile) -> Option<std::ops::RangeInclusive<i64>> {
    whole_minutes(
        window.start.0,
        window.end.0,
        file.time_range.start.0,
        file.time_range.end.0,
    )
}
