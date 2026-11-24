// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The logs of one stream in a window, grouped into message patterns and compared with a baseline
//! stretch: for the Patterns view of Logs (`POST /web/logs/patterns`), the agent's
//! `analyze_log_patterns`, and the evidence of an incident's root-cause analysis.
//!
//! A stretch with more logs than [`ROW_CAP`] is sampled across its whole length (the newest logs
//! of each of [`SAMPLE_PARTS`] equal parts), and the counts are scaled to the stretch's total,
//! which a count query reads. The window and the baseline are grouped in one reading, so a
//! pattern is the same in both.

mod change;
mod grouping;
mod report;
mod stretch;
mod tokens;

#[cfg(test)]
mod tests;

pub use change::Change;
use domain::stream::Schema;
use kernel::{Error, Result, ids::Id, time::TimeRange};
pub use report::{BaselineStretch, LevelCount, Pattern, PatternReport, Span};
pub use tokens::{Token, Wild};

use self::{report::report, stretch::Reader};
use super::query::{QueryService, baseline::Baseline};

/// The most logs read of one stretch.
pub const ROW_CAP: usize = 5_000;

/// A stretch with more logs than [`ROW_CAP`] is read in this many equal parts.
const SAMPLE_PARTS: i64 = 4;

/// The most patterns a report lists.
pub const MAX_LIMIT: usize = 200;

/// The fields a log says its message in, in the order they are looked for: those of the Logs page
/// first, so the patterns are of the field the page reads.
pub const MESSAGE_FIELDS: &[&str] = &[
    "message", "body", "summary", "log", "event", "error", "msg", "content", "text",
];

/// The fields a log says its level in, in the order they are looked for.
const LEVEL_FIELDS: &[&str] = &[
    "level",
    "severity_text",
    "severity",
    "log.level",
    "levelname",
    "log_level",
    "loglevel",
    "lvl",
];

/// What to group: the logs of a stream that hold the message field and meet the filter.
pub struct PatternQuery<'a> {
    pub org_id: &'a Id,
    pub stream: &'a str,
    pub message: &'a str,
    pub level: Option<&'a str>,
    /// ` AND (…)`, already checked, or nothing.
    pub filter: String,
    pub window: TimeRange,
    pub baseline: Baseline,
    pub limit: usize,
    /// Each pattern also tells its words, for a page that draws them.
    pub tokens: bool,
}

/// The patterns of the window, compared with the baseline the query asks for.
pub async fn analyze(
    query_service: &QueryService,
    query: &PatternQuery<'_>,
) -> Result<PatternReport> {
    let reader = Reader {
        query: query_service,
        org_id: query.org_id,
        stream: query.stream,
        message: query.message,
        level: query.level,
        filter: &query.filter,
    };

    let current = reader.stretch(query.window).await?;

    let baseline = match query.baseline.range(query.window) {
        Some(range) => Some(reader.stretch(range).await?),
        None => None,
    };

    Ok(report(query, &current, baseline.as_ref()))
}

/// The field a stream says its message in: `asked`, which must be one of its fields, or the first
/// of [`MESSAGE_FIELDS`] it has.
pub fn message_field(schema: &Schema, stream: &str, asked: Option<&str>) -> Result<String> {
    match asked.map(str::trim).filter(|field| !field.is_empty()) {
        Some(field) if schema.fields.iter().any(|known| known.name == field) => {
            Ok(field.to_owned())
        }
        Some(field) => Err(Error::invalid(format!(
            "stream `{stream}` has no field `{field}`"
        ))),
        None => find_field(schema, MESSAGE_FIELDS)
            .map(str::to_owned)
            .ok_or_else(|| {
                Error::invalid(format!(
                    "stream `{stream}` has none of the message fields {}: name the field that \
                     holds the message",
                    MESSAGE_FIELDS.join(", ")
                ))
            }),
    }
}

/// The field a stream says its level in, other than its message field.
pub fn level_field(schema: &Schema, message: &str) -> Option<String> {
    find_field(schema, LEVEL_FIELDS)
        .filter(|level| *level != message)
        .map(str::to_owned)
}

/// Letters and digits alone: `log.level`, `log_level` and `logLevel` are one name.
fn plain(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

/// The stream's field for a meaning: one named exactly as one of `names`, in their order, or else
/// one whose plain name is.
fn find_field<'a>(schema: &'a Schema, names: &[&str]) -> Option<&'a str> {
    let exact = names
        .iter()
        .find_map(|name| schema.fields.iter().find(|field| field.name == *name));

    let loose = || {
        names.iter().find_map(|name| {
            let wanted = plain(name);

            schema
                .fields
                .iter()
                .find(|field| plain(&field.name) == wanted)
        })
    };

    exact.or_else(loose).map(|field| field.name.as_str())
}
