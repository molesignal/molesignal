// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `analyze_log_patterns`: the logs of one stream in a window, grouped into message patterns and
//! compared with a baseline stretch.
//!
//! The logs are read with the same stream checks and query path as `query_logs`. A stretch with
//! more logs than [`ROW_CAP`] is sampled across its whole length (the newest logs of each of
//! [`SAMPLE_PARTS`] equal parts), and the counts are scaled to the stretch's total, which a count
//! query reads. The window and the baseline are grouped in one reading, so a pattern is the same
//! in both.

mod change;
mod grouping;
mod summary;
mod tokens;

use domain::{
    iam::IamContext,
    query::QueryLanguage,
    stream::{Schema, StreamType},
};
use kernel::{
    Error, Result,
    ids::Id,
    time::{TimeRange, TimestampMicros},
};
use serde::Deserialize;
use serde_json::Value;
use tool_runtime::ToolResult;

use self::summary::{Shape, report};
use super::{
    super::{ToolRuntime, common::*},
    baseline::Baseline,
    validated_filter,
};

/// The most logs read of one stretch.
const ROW_CAP: usize = 5_000;

/// A stretch with more logs than [`ROW_CAP`] is read in this many equal parts.
const SAMPLE_PARTS: i64 = 4;

const DEFAULT_LIMIT: usize = 30;
const MAX_LIMIT: usize = 100;

/// The fields a log says its message in, in the order they are looked for: those of the Logs page
/// first, so the tool reads the field the page reads.
const MESSAGE_FIELDS: &[&str] = &[
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Args {
    stream: String,
    time_range: TimeRangeArg,
    #[serde(default)]
    filter_sql: Option<String>,
    #[serde(default)]
    field: Option<String>,
    #[serde(default)]
    baseline: Baseline,
    #[serde(default)]
    limit: Option<usize>,
}

pub(super) async fn analyze(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: Args = parse_args(arguments)?;
    let stream = require_stream(runtime, &auth.org_id, &args.stream, StreamType::LOGS).await?;

    let message = match args
        .field
        .as_deref()
        .map(str::trim)
        .filter(|field| !field.is_empty())
    {
        Some(field) => require_field(&stream.schema, field)?.to_string(),
        None => find_field(&stream.schema, MESSAGE_FIELDS)
            .ok_or_else(|| {
                Error::invalid(format!(
                    "stream `{}` has none of the message fields {}: name the field that holds the \
                     message with `field`",
                    args.stream,
                    MESSAGE_FIELDS.join(", ")
                ))
            })?
            .to_string(),
    };

    let level = find_field(&stream.schema, LEVEL_FIELDS)
        .filter(|level| *level != message)
        .map(str::to_string);

    let window = time_range(args.time_range)?;

    let reader = Reader {
        runtime,
        org_id: &auth.org_id,
        stream: &args.stream,
        message: &message,
        level: level.as_deref(),
        filter: validated_filter(args.filter_sql.as_deref())?,
    };

    let current = reader.stretch(window).await?;

    let baseline = match args.baseline.range(window) {
        Some(range) => Some(reader.stretch(range).await?),
        None => None,
    };

    let shape = Shape {
        stream: &args.stream,
        field: &message,
        level_field: level.as_deref(),
        baseline: args.baseline,
        limit: args.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT),
    };

    Ok(ToolResult::json(report(
        &shape,
        &current,
        baseline.as_ref(),
    )))
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

/// One log read: when it happened, what it says and its level.
struct LogRow {
    at: Option<i64>,
    message: String,
    level: Option<String>,
}

impl LogRow {
    fn says_something(&self) -> bool {
        !self.message.trim().is_empty()
    }
}

/// The logs of one stretch: how many have a message in all, and those read.
struct Stretch {
    range: TimeRange,
    total: u64,
    rows: Vec<LogRow>,
}

impl Stretch {
    fn sampled(&self) -> bool {
        (self.rows.len() as u64) < self.total
    }

    fn said(&self) -> usize {
        self.rows.iter().filter(|row| row.says_something()).count()
    }
}

struct Reader<'a> {
    runtime: &'a ToolRuntime,
    org_id: &'a Id,
    stream: &'a str,
    message: &'a str,
    level: Option<&'a str>,
    /// ` AND (…)` from `filterSql`, or nothing.
    filter: String,
}

impl Reader<'_> {
    async fn stretch(&self, range: TimeRange) -> Result<Stretch> {
        let total = self.count(range).await?;

        if total == 0 {
            return Ok(Stretch {
                range,
                total,
                rows: Vec::new(),
            });
        }

        let parts = if total <= ROW_CAP as u64 {
            1
        } else {
            SAMPLE_PARTS
        };

        let per_part = ROW_CAP / parts as usize;
        let mut rows = Vec::new();

        for part in sample_parts(range, parts) {
            rows.extend(self.read(part, per_part).await?);
        }

        // Newest first, so the first log of a pattern is its latest.
        rows.sort_by_key(|row| std::cmp::Reverse(row.at));

        Ok(Stretch {
            range,
            total: total.max(rows.len() as u64),
            rows,
        })
    }

    fn where_clause(&self) -> String {
        format!(
            " WHERE {} IS NOT NULL{}",
            quote_ident(self.message),
            self.filter
        )
    }

    async fn count(&self, range: TimeRange) -> Result<u64> {
        let statement = format!(
            "SELECT COUNT(*) AS count FROM {}{}",
            quote_ident(self.stream),
            self.where_clause()
        );

        let result = run_query(
            self.runtime,
            self.org_id,
            QueryLanguage::Sql,
            statement,
            range,
            Some((self.stream, StreamType::LOGS)),
            1,
        )
        .await?;

        Ok(result
            .rows
            .first()
            .and_then(|row| row.first())
            .and_then(Value::as_u64)
            .unwrap_or_default())
    }

    async fn read(&self, range: TimeRange, limit: usize) -> Result<Vec<LogRow>> {
        let mut columns = vec![quote_ident("_timestamp"), quote_ident(self.message)];

        if let Some(level) = self.level {
            columns.push(quote_ident(level));
        }

        let statement = format!(
            "SELECT {} FROM {}{} ORDER BY {} DESC LIMIT {limit}",
            columns.join(", "),
            quote_ident(self.stream),
            self.where_clause(),
            quote_ident("_timestamp"),
        );

        let result = run_query(
            self.runtime,
            self.org_id,
            QueryLanguage::Sql,
            statement,
            range,
            Some((self.stream, StreamType::LOGS)),
            limit,
        )
        .await?;

        Ok(result
            .rows
            .iter()
            .map(|row| LogRow {
                at: row.first().and_then(Value::as_i64),
                message: row.get(1).map(text_of).unwrap_or_default(),
                level: self
                    .level
                    .and(row.get(2))
                    .map(text_of)
                    .map(|level| level.trim().to_uppercase())
                    .filter(|level| !level.is_empty()),
            })
            .collect())
    }
}

fn text_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// `range` in `parts` equal closed parts, the last one ending where the range does.
fn sample_parts(range: TimeRange, parts: i64) -> Vec<TimeRange> {
    let start = range.start.0;
    let span = range.end.0.saturating_sub(start);

    (0..parts)
        .map(|part| {
            let from = start.saturating_add(span / parts * part);

            let to = if part + 1 == parts {
                range.end.0
            } else {
                start
                    .saturating_add(span / parts * (part + 1))
                    .saturating_sub(1)
            };

            TimeRange::new(TimestampMicros(from), TimestampMicros(to.max(from)))
        })
        .collect()
}

#[cfg(test)]
mod tests;
