// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The two sets of Spans compared: those of a query that are chosen (slow, failed, or both) and
//! the others of the same query, each read as a sample of its first rows.

use std::future::Future;

use domain::{
    query::QueryResult,
    stream::{FieldDef, FieldType, Schema, StreamDefinition},
};
use kernel::{Error, Result};
use serde_json::Value;

use super::{Comparison, Rows, compare};
use crate::app::{
    query::statement::{Named, ident},
    trace::reading::{duration_expression, failed_expression},
};

/// The most fields read of a Span.
const MOST_FIELDS: usize = 120;

/// Fields of a Span that tell nothing about a set of requests: ids, moments, how long it took
/// (what is chosen by), and what the server writes about the Span itself.
const LEFT_OUT: [&str; 18] = [
    "trace_id",
    "span_id",
    "parent_span_id",
    "trace_state",
    "trace_flags",
    "start_time_unix_nano",
    "end_time_unix_nano",
    "duration_ns",
    "dropped_attributes_count",
    "dropped_events_count",
    "dropped_links_count",
    "schema_version",
    "semantic_conventions_version",
    "partial",
    "late",
    "duplicate",
    "conflict",
    "_timestamp",
];

/// Which requests are chosen: those that took at least, or less than, so long, those that
/// failed, or those that meet all of what is given.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    /// Nanoseconds, inclusive.
    pub min_duration: Option<i64>,
    /// Nanoseconds, exclusive.
    pub max_duration: Option<i64>,
    pub errors: bool,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.min_duration.is_none() && self.max_duration.is_none() && !self.errors
    }

    /// Whether the choice can choose anything, whatever the stream.
    pub fn check(&self) -> Result<()> {
        if self.is_empty() {
            return Err(Error::invalid(
                "say which requests to compare: a duration range, the failed ones, or both",
            ));
        }

        if let (Some(min), Some(max)) = (self.min_duration, self.max_duration)
            && min >= max
        {
            return Err(Error::invalid(
                "the shortest duration chosen must be below the longest",
            ));
        }

        Ok(())
    }

    /// What a chosen Span meets, as SQL.
    pub fn clause(&self, schema: &Schema) -> Result<String> {
        self.check()?;

        let duration = duration_expression(schema);
        let mut parts = Vec::new();

        if let Some(min) = self.min_duration {
            parts.push(format!("{duration} >= {min}"));
        }

        if let Some(max) = self.max_duration {
            parts.push(format!("{duration} < {max}"));
        }

        if self.errors {
            let failed = failed_expression(schema).ok_or_else(|| {
                Error::invalid("the trace stream keeps no status to tell failed requests by")
            })?;

            parts.push(format!("{failed} = 1"));
        }

        Ok(format!("({})", parts.join(" AND ")))
    }
}

/// What to compare.
pub struct SpanComparison<'a> {
    pub definition: &'a StreamDefinition,
    /// What every Span of both sets meets: the query's own conditions.
    pub clauses: Vec<String>,
    pub selection: &'a Selection,
    /// The most rows read of each set.
    pub limit: usize,
}

/// The fields of a Span whose values can tell sets of requests apart.
pub fn candidates(schema: &Schema) -> Vec<&FieldDef> {
    schema
        .fields
        .iter()
        .filter(|field| {
            !field.encrypted
                && !matches!(field.data_type, FieldType::Timestamp | FieldType::Json)
                && !field.name.starts_with("molesignal.")
                && !LEFT_OUT.contains(&field.name.as_str())
        })
        .take(MOST_FIELDS)
        .collect()
}

/// Reads both sets with `run` and compares them.
pub async fn compare_spans<F, Fut>(asked: SpanComparison<'_>, run: F) -> Result<Comparison>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<QueryResult>>,
{
    let schema = &asked.definition.schema;
    let chosen = asked.selection.clause(schema)?;
    // The status is what failed requests are chosen by: it would set them apart every time.
    let fields = candidates(schema)
        .into_iter()
        .filter(|field| !(asked.selection.errors && field.name == "status_code"))
        .collect::<Vec<_>>();

    if fields.is_empty() {
        return Ok(compare(&[], &Vec::new(), &Vec::new(), false));
    }

    let reading = |condition: String| {
        statement(
            &asked.definition.name,
            &fields,
            asked.clauses.iter().cloned().chain([condition]),
            asked.limit,
        )
    };

    let selected = reading(chosen.clone());
    let rest = reading(format!("NOT COALESCE({chosen}, FALSE)"));
    let (selected, rest) = tokio::try_join!(run(selected), run(rest))?;

    let selected = rows(&selected, fields.len());
    let rest = rows(&rest, fields.len());
    let sampled = selected.len() >= asked.limit || rest.len() >= asked.limit;
    let names = fields
        .iter()
        .map(|field| field.name.clone())
        .collect::<Vec<_>>();

    Ok(compare(&names, &selected, &rest, sampled))
}

/// The fields' values as text, each named apart from every field, of the first `limit` Spans
/// that meet every clause.
pub(super) fn statement(
    table: &str,
    fields: &[&FieldDef],
    clauses: impl Iterator<Item = String>,
    limit: usize,
) -> String {
    let columns = fields
        .iter()
        .enumerate()
        .map(|(index, field)| format!("{} AS __diff_{index}", Named::Field(field).text()))
        .collect::<Vec<_>>();

    let clauses = clauses.collect::<Vec<_>>();
    let filter = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };

    format!(
        "SELECT {} FROM \"{}\"{filter} LIMIT {limit}",
        columns.join(", "),
        ident(table)
    )
}

fn rows(result: &QueryResult, width: usize) -> Rows {
    result
        .rows
        .iter()
        .map(|row| {
            (0..width)
                .map(|index| match row.get(index) {
                    None | Some(Value::Null) => None,
                    Some(Value::String(text)) => Some(text.clone()),
                    Some(other) => Some(other.to_string()),
                })
                .collect()
        })
        .collect()
}
