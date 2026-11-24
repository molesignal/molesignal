// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The logs of one stretch: how many hold a message, and those read, sampled across the stretch
//! when there are more than [`ROW_CAP`].

use domain::{
    query::{QueryLanguage, QueryRequest, StreamHint},
    stream::StreamType,
};
use kernel::{
    Result,
    ids::Id,
    time::{TimeRange, TimestampMicros},
};
use serde_json::Value;

use super::{ROW_CAP, SAMPLE_PARTS};
use crate::app::query::{
    QueryService,
    statement::{count, ident},
};

/// One log read: when it happened, what it says and its level.
pub(super) struct LogRow {
    pub(super) at: Option<i64>,
    pub(super) message: String,
    pub(super) level: Option<String>,
}

impl LogRow {
    pub(super) fn says_something(&self) -> bool {
        !self.message.trim().is_empty()
    }
}

/// The logs of one stretch: how many have a message in all, and those read.
pub(super) struct Stretch {
    pub(super) range: TimeRange,
    pub(super) total: u64,
    pub(super) rows: Vec<LogRow>,
}

impl Stretch {
    pub(super) fn sampled(&self) -> bool {
        (self.rows.len() as u64) < self.total
    }

    pub(super) fn said(&self) -> usize {
        self.rows.iter().filter(|row| row.says_something()).count()
    }
}

pub(super) struct Reader<'a> {
    pub(super) query: &'a QueryService,
    pub(super) org_id: &'a Id,
    pub(super) stream: &'a str,
    pub(super) message: &'a str,
    pub(super) level: Option<&'a str>,
    /// ` AND (…)`, or nothing.
    pub(super) filter: &'a str,
}

impl Reader<'_> {
    pub(super) async fn stretch(&self, range: TimeRange) -> Result<Stretch> {
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
            " WHERE \"{}\" IS NOT NULL{}",
            ident(self.message),
            self.filter
        )
    }

    async fn run(
        &self,
        statement: String,
        range: TimeRange,
        limit: usize,
    ) -> Result<Vec<Vec<Value>>> {
        let result = self
            .query
            .run(QueryRequest {
                org_id: self.org_id.clone(),
                language: QueryLanguage::Sql,
                statement,
                time_range: range,
                stream: Some(StreamHint {
                    name: self.stream.to_owned(),
                    stream_type: StreamType::LOGS,
                }),
                limit: Some(limit),
                step: None,
                federation_clusters: Vec::new(),
            })
            .await?;

        Ok(result.rows)
    }

    async fn count(&self, range: TimeRange) -> Result<u64> {
        let statement = format!(
            "SELECT COUNT(*) AS count FROM \"{}\"{}",
            ident(self.stream),
            self.where_clause()
        );

        let rows = self.run(statement, range, 1).await?;
        Ok(rows
            .first()
            .and_then(|row| row.first())
            .map(count)
            .unwrap_or_default())
    }

    async fn read(&self, range: TimeRange, limit: usize) -> Result<Vec<LogRow>> {
        let mut columns = vec![
            "\"_timestamp\"".to_owned(),
            format!("\"{}\"", ident(self.message)),
        ];

        if let Some(level) = self.level {
            columns.push(format!("\"{}\"", ident(level)));
        }

        let statement = format!(
            "SELECT {} FROM \"{}\"{} ORDER BY \"_timestamp\" DESC LIMIT {limit}",
            columns.join(", "),
            ident(self.stream),
            self.where_clause(),
        );

        let rows = self.run(statement, range, limit).await?;

        Ok(rows
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
pub(super) fn sample_parts(range: TimeRange, parts: i64) -> Vec<TimeRange> {
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
